//! 012 US5 (FR-009 to FR-012, FR-026; research R-8, R-9): what a coding assistant needs beside
//! the message box. Images attached to a session's message (checked by their magic bytes, up to
//! 5 MiB and 4 per message, kept with the session by their sha-256), the waiting messages
//! edited, removed or sent now, and the session's web access switched on or off when the
//! runtime can do that.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::Studio;
use super::solo::ASSISTANT;
use crate::agent::{self, prompts};
use crate::conversation::events::Event as CEvent;
use crate::conversation::solo::{AttachmentRec, SoloEvent};
use crate::run::engine::Ctx;

/// The largest image attached (R-8).
pub const MAX_IMAGE: usize = 5 * 1024 * 1024;
/// The most images one message carries (R-8).
pub const MAX_IMAGES: usize = 4;

/// The image type and file extension of `bytes`, by their magic bytes: png, jpeg, gif or webp.
pub fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

fn ext_of(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        _ => "webp",
    }
}

/// Where an attached image is kept: `conversations/<s>/attachments/<sha>.<ext>`.
pub fn attachment_path(conv_dir: &Path, a: &AttachmentRec) -> PathBuf {
    conv_dir
        .join("attachments")
        .join(format!("{}.{}", a.sha, ext_of(&a.media_type)))
}

fn is_sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Studio {
    /// What the session's assistant runtime can do (FR-010a).
    fn assistant_caps(&self, conv: &str) -> Result<agent::Capabilities, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        c.state.solo.as_ref().ok_or("not a session")?;
        let p = self.project(&c.state.project)?;
        let name = c
            .state
            .agent
            .clone()
            .unwrap_or_else(|| ASSISTANT.to_string());
        let lib = self.library(Some(&p.path));
        let def = lib
            .agent(&name)
            .ok_or_else(|| format!("there is no agent {name}"))?;
        Ok(agent::registry::capabilities(&def.runtime, &self.settings))
    }

    /// A session still open to messages.
    fn session_open(&self, conv: &str) -> Result<(), String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.as_ref().ok_or("not a session")?;
        if s.closed.is_some() {
            return Err("the session is closed".into());
        }
        Ok(())
    }

    /// 012 (R-8): `POST /api/sessions/<s>/attachments`: the image kept with the session under
    /// its sha-256; refused when it is not an image by its magic bytes, is over 5 MiB, or the
    /// assistant's runtime cannot take images.
    pub fn solo_attach(
        &mut self,
        _ctx: &Ctx,
        conv: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<Value, String> {
        self.session_open(conv)?;
        if !self.assistant_caps(conv)?.images {
            return Err("This assistant's runtime cannot take images".into());
        }
        if bytes.len() > MAX_IMAGE {
            return Err("an image can be up to 5 MiB".into());
        }
        let (media_type, ext) =
            image_type(bytes).ok_or("only images can be attached: png, jpeg, gif or webp")?;
        let sha = sha256_hex(bytes);
        let dir = self
            .convs
            .get(conv)
            .ok_or("there is no session")?
            .dir
            .join("attachments");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = dir.join(format!("{sha}.{ext}"));
        if !file.exists() {
            std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        let name = Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or("image")
            .to_string();
        let rec = AttachmentRec {
            sha: sha.clone(),
            name,
            media_type: media_type.to_string(),
            size: bytes.len() as u64,
        };
        let meta = dir.join(format!("{sha}.json"));
        std::fs::write(
            &meta,
            serde_json::to_string(&rec).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", meta.display()))?;
        Ok(
            json!({ "id": sha, "sha": rec.sha, "media_type": rec.media_type, "size": rec.size, "name": rec.name }),
        )
    }

    /// The kept image `sha` of the session: its record. Only its name is taken from the record
    /// file: the type comes from the kept bytes' magic, and the bytes must hash to `sha`, so a
    /// file written beside them by anything else is never served or handed on (Phase 9 review).
    fn attachment_rec(&self, conv: &str, sha: &str) -> Result<AttachmentRec, String> {
        const GONE: &str = "there is no such attachment: attach the image again";
        if !is_sha(sha) {
            return Err("there is no such attachment".into());
        }
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let dir = c.dir.join("attachments");
        let name = std::fs::read_to_string(dir.join(format!("{sha}.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<AttachmentRec>(&t).ok())
            .map(|r| r.name)
            .ok_or(GONE)?;
        for ext in ["png", "jpg", "gif", "webp"] {
            let Ok(bytes) = std::fs::read(dir.join(format!("{sha}.{ext}"))) else {
                continue;
            };
            let Some((media_type, e)) = image_type(&bytes) else {
                continue;
            };
            if e != ext || bytes.len() > MAX_IMAGE || sha256_hex(&bytes) != sha {
                continue;
            }
            return Ok(AttachmentRec {
                sha: sha.to_string(),
                name,
                media_type: media_type.to_string(),
                size: bytes.len() as u64,
            });
        }
        Err(GONE.into())
    }

    /// The kept image `sha`, checked (for a waiting message as it is sent).
    pub(super) fn attachment_checked(
        &self,
        conv: &str,
        sha: &str,
    ) -> Result<AttachmentRec, String> {
        self.attachment_rec(conv, sha)
    }

    /// `GET /api/sessions/<s>/attachments/<sha>`: where the image is, and its type.
    pub fn solo_attachment(&self, conv: &str, sha: &str) -> Result<Value, String> {
        let rec = self.attachment_rec(conv, sha)?;
        let c = self.convs.get(conv).ok_or("there is no session")?;
        Ok(json!({ "path": attachment_path(&c.dir, &rec), "media_type": rec.media_type }))
    }

    /// 012 (R-8): a message with attached images: refused before anything is sent when the
    /// runtime cannot take images, an image is unknown, or there are more than 4.
    pub fn solo_say_with(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
        ids: &[String],
    ) -> Result<Value, String> {
        self.session_open(conv)?;
        if ids.len() > MAX_IMAGES {
            return Err(format!("a message can carry up to {MAX_IMAGES} images"));
        }
        if !ids.is_empty() && !self.assistant_caps(conv)?.images {
            return Err("This assistant's runtime cannot take images".into());
        }
        let recs = ids
            .iter()
            .map(|id| self.attachment_rec(conv, id))
            .collect::<Result<Vec<_>, _>>()?;
        self.solo_turn(ctx, conv, text, refs, None, recs)
    }

    /// 012 (R-9): `POST /api/sessions/<s>/queue/<id>`: a waiting message edited, removed, or sent
    /// now ("Send now" after a restart or a failure: the engine sends it, its files read then).
    pub fn solo_queue(
        &mut self,
        conv: &str,
        id: &str,
        text: Option<String>,
        remove: bool,
        send: bool,
    ) -> Result<Value, String> {
        self.session_open(conv)?;
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.as_ref().ok_or("not a session")?;
        let Some(pos) = s.queue.iter().position(|w| w.id == id) else {
            return Err("there is no waiting message with that id: it was sent or removed".into());
        };
        let c = self.conv_mut(conv)?;
        if remove {
            c.append(CEvent::Solo(SoloEvent::QueueRemoved { id: id.to_string() }))?;
        } else if let Some(t) = text {
            let t = t.trim().to_string();
            if t.is_empty() {
                return Err("a waiting message cannot be empty: remove it instead".into());
            }
            c.append(CEvent::Solo(SoloEvent::QueueEdited {
                id: id.to_string(),
                text: t,
            }))?;
        } else if send {
            if pos != 0 {
                return Err("messages go in the order typed: send the first one waiting".into());
            }
            // The engineer's go-ahead: the hold after a restart or a failure is lifted.
            c.queue_hold = false;
            c.queue_go = true;
            c.slot_wait = None;
        } else {
            return Err("say what to do with the waiting message: text, remove or send".into());
        }
        c.tick();
        Ok(json!({}))
    }

    /// 012 (FR-009): `POST /api/sessions/<s>/web`: refused when the runtime cannot switch web
    /// access off.
    pub fn solo_web(&mut self, _ctx: &Ctx, conv: &str, on: bool) -> Result<Value, String> {
        self.session_open(conv)?;
        if !self.assistant_caps(conv)?.web_off {
            return Err("This assistant's runtime cannot switch web access off".into());
        }
        let c = self.conv_mut(conv)?;
        if c.state.solo.as_ref().is_some_and(|s| s.web != on) {
            c.append(CEvent::Solo(SoloEvent::WebSet { on }))?;
            c.tick();
        }
        Ok(json!({}))
    }
}

/// SHA-256 of `data`, in lower-case hex (FIPS 180-4), for naming kept images by their content.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

impl Studio {
    /// What the session's assistant runtime can do, for its view (defaults when unknown).
    pub(super) fn solo_caps(&self, c: &super::Conv) -> agent::Capabilities {
        let name = c
            .state
            .agent
            .clone()
            .unwrap_or_else(|| ASSISTANT.to_string());
        self.project(&c.state.project)
            .ok()
            .and_then(|p| {
                self.library(Some(&p.path))
                    .agent(&name)
                    .map(|d| d.runtime.clone())
            })
            .map(|r| agent::registry::capabilities(&r, &self.settings))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn images_are_known_by_their_magic_bytes() {
        assert_eq!(
            image_type(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0])
                .unwrap()
                .0,
            "image/png"
        );
        assert_eq!(
            image_type(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().0,
            "image/jpeg"
        );
        assert_eq!(image_type(b"GIF89a...").unwrap().0, "image/gif");
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 ").unwrap().0, "image/webp");
        assert!(
            image_type(b"<svg xmlns=").is_none(),
            "no svg: it is a script carrier"
        );
        assert!(image_type(b"RIFF\0\0\0\0WAVE").is_none());
    }
}
