//! 015 (2026-11-24 amendment, the engineer): Auto runs everything except at its fixed limits.
//! The ten requests Fallow refused in the engineer's real chat (a session in Auto, reading and
//! writing scratch files in `/tmp`, reading the project through `~`) are replayed as the
//! assistant's, exactly as the runtime sent them: all ten are allowed, with no card. What
//! changes the engineer's folder, a push onto the target and a control file still stop.
//!
//! One test in its own binary: it points `HOME` at a temporary folder holding a fake
//! `~/repos/nvwa-fc`, the engineer's project, so `~` never reads the real home folder.

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use fallow::agent::{PermissionRequest, RequestKind};
use fallow::studio::Req;
use fallow::studio::judge;
use fallow::studio::mode::{self, AssistantCx, ChatMode, Decision, Limit, Target};

/// The requests of the engineer's chat (conversation c6571178e), `{WT}` standing for the
/// session's working copy: (refusal, kind, command or path).
const TEN: &[(&str, &str, &str)] = &[
    (
        "r1",
        "run",
        r###"find ~/repos/nvwa-fc -maxdepth 4 -name python -path "*bin*" 2>/dev/null | head; which uv python3.12 python3.11 2>/dev/null"###,
    ),
    (
        "r2",
        "run",
        r###"cd "{WT}/agentic-workflow/classifier/output"; for k in 'Zendingen die grond' 'Alle landen waar' 'origine niet Nederland' 'De inspectieplichtige' 'Product- land' 'Prunus (15' 'annuum\*' 'm.i.v. 25-02' 'van geregistreerde bedrijven' 'groter of gelijk'; do echo "=== $k"; grep -F "$k" /tmp/cropscan/shard2.tsv >/dev/null; awk -F'\t' -v k="$k" '$1 ~ k {print $4"\t"$5; exit}' /tmp/cropscan/shard2.tsv; done"###,
    ),
    (
        "r3",
        "run",
        r###"cut -f1,2,4 /tmp/cropscan/shard3.tsv | awk -F'\t' '{printf "%d|%s|%s|%s\n", NR,$2,substr($1,1,260),$3}' | sed -n '151,290p' | sed 's/_exporteisen[^|]*_ast.yml//'"###,
    ),
    ("r4", "edit", r###"/tmp/cropscan/result1.md"###),
    (
        "r5",
        "run",
        r###"cd /tmp/cropscan; tail -n +2 shard4.tsv | wc -l; tail -3 shard4.tsv | cut -f1,2; awk -F'\t' 'length($1)>200{print NR": "$1"\t"$2"\t"$4}' shard4.tsv; grep -P '^(Plants of Pelargonium|Plants of Rubus|Planten bestemd voor opplant, m.u.v. zaden, van Capsicum)' shard4.tsv | cut -f1,4"###,
    ),
    (
        "r6",
        "run",
        r###"cd /tmp/cropscan; for p in 'GECONDITIONEERDE TEELT' 'Materiaal afkomstig' 'Heliconia (origine' 'Fraxinus excelsior (Invoer' 'Planten van producten waarvoor' 'Nederlandse boomkwekerij' 'Pelargonium (voortkwekingsmateriaal) zie' 'Phalaenopsis spp. (voortkwekingsmateriaal) origine Duitsland'; do grep -F "$p" shard4.tsv | cut -f1,4,5 | cut -c1-400; done"###,
    ),
    (
        "r7",
        "run",
        r###"cut -f1,2,4 /tmp/cropscan/shard5.tsv | awk -F'\t' '{print NR": "substr($1,1,230)" |"$2}' | tail -n +151; sed -n 34p /tmp/cropscan/shard5.tsv | cat -A | head -3"###,
    ),
    ("r8", "edit", r###"/tmp/cropscan/result2.md"###),
    (
        "r9",
        "run",
        r###"cd /tmp/cropscan; awk -F'\t' 'NR>1 && ($1 ~ /^Carya \(|Cineariae|anders dan|Forest trees|asterisk|Malvaceous|Producten op basis|idem Fusarium|indien uit|TPS|origines|List of High|niet is toegestaan|^SOLANUM|Stolonen|stolonen|Hybriden/) {print NR": "$1"\t"$4"\t"$5}' shard6.tsv"###,
    ),
    ("r10", "edit", r###"/tmp/cropscan/result5.md"###),
];

/// Two more the same chat refused later, the same way.
const LATER: &[(&str, &str, &str)] = &[
    (
        "r11",
        "run",
        r###"cd /tmp/cropscan; ls; for f in result3.md result4.md result6.md; do echo "== $f"; grep -c '^|' $f; grep '^|' $f | sed -n '1,4p' | cut -c1-250; done; cd - >/dev/null; git check-ignore -v agentic-workflow/classifier/output/crops_audit.tsv agentic-workflow/classifier/output/crops_audit.md; git ls-files agentic-workflow/classifier/output | grep -v '_ast.yml$' | head"###,
    ),
    (
        "r12",
        "run",
        r###"mkdir -p _cropscan && cp /tmp/cropscan/* _cropscan/ && ls _cropscan; git check-ignore -v _cropscan/x.tsv _cropscan/x.md _cropscan/x.jsonl agentic-workflow/classifier/output/crops_audit.tsv agentic-workflow/classifier/output/crops_audit.md; git ls-files agentic-workflow/classifier/output | grep -v '_ast.yml$' | head"###,
    ),
];

fn request(kind: &str, what: &str) -> PermissionRequest {
    PermissionRequest {
        kind: if kind == "run" {
            RequestKind::Run
        } else {
            RequestKind::Edit
        },
        command: (kind == "run").then(|| what.to_string()),
        path: (kind != "run").then(|| what.to_string()),
        url: None,
        query: None,
        tool: if kind == "run" { "Bash" } else { "Write" }.into(),
    }
}

struct Fixture {
    wt: PathBuf,
    folder: PathBuf,
    protect: Vec<String>,
    control: Vec<String>,
}

impl Fixture {
    fn decide(&self, mode: ChatMode, kind: &str, what: &str, target: Option<&Target>) -> Decision {
        let ctx = judge::Ctx {
            wt: &self.wt,
            branch: "fallow/solo/x",
            folder: &self.folder,
            protect: &self.protect,
        };
        mode::assistant(
            mode,
            &request(kind, what),
            &AssistantCx {
                judge: &ctx,
                control_files: &self.control,
                gates: &[],
                approved: &[],
                target,
            },
            |_| None,
        )
    }
}

fn held_at(d: &Decision) -> Option<Limit> {
    match d {
        Decision::Hold { limit, .. } => Some(*limit),
        _ => None,
    }
}

#[test]
fn the_engineer_s_ten_refusals_run_in_auto_and_the_limits_still_stop() {
    let root = scratch("us15-replay");
    // A temporary home, with the engineer's project at `~/repos/nvwa-fc`: short, since the
    // service's socket goes below it (`SUN_LEN`).
    let home = Path::new("/tmp").join(format!("fallow-h{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("repos")).unwrap();
    // SAFETY: the only test of this binary, set before any thread or child process starts.
    unsafe { std::env::set_var("HOME", &home) };
    let folder = repo_named(&home.join("repos"), "nvwa-fc");
    let folder = folder.canonicalize().unwrap();
    let wt = root.join("copy");
    std::fs::create_dir_all(wt.join("agentic-workflow/classifier/output")).unwrap();
    let wt = wt.canonicalize().unwrap();
    let control = vec![
        ".claude/settings.json".to_string(),
        ".claude/settings.local.json".to_string(),
    ];
    let mut protect: Vec<String> = fallow::agent::rights::CONTROL_GLOBS
        .iter()
        .map(|g| g.to_string())
        .collect();
    protect.extend(control.clone());
    protect.extend([".git".to_string(), ".git/**".to_string()]);
    let fx = Fixture {
        wt: wt.clone(),
        folder: folder.clone(),
        protect,
        control,
    };
    let wt_text = wt.display().to_string();
    let target = Target {
        name: "main".into(),
        protected: true,
        also: Vec::new(),
    };

    // 1. All ten, and the two after them, as written: allowed in Auto, no card.
    for (id, kind, what) in TEN.iter().chain(LATER) {
        let what = what.replace("{WT}", &wt_text);
        assert_eq!(
            fx.decide(ChatMode::Auto, kind, &what, Some(&target)),
            Decision::Allow,
            "{id}: {what}"
        );
    }

    // 2. What still stops in Auto. The engineer's folder changed waits for the engineer.
    let f = folder.display().to_string();
    for cmd in [
        format!("echo x > {f}/src/a.py"),
        format!("rm {f}/x"),
        format!("cd {f} && git checkout ."),
        format!("cd {f}; rm -rf build"),
        "echo x > ~/repos/nvwa-fc/src/a.py".to_string(),
        "cd ~/repos/nvwa-fc && npm install".to_string(),
        format!("cp /tmp/cropscan/result1.md {f}/docs/result.md"),
    ] {
        let d = fx.decide(ChatMode::Auto, "run", &cmd, Some(&target));
        assert_eq!(held_at(&d), Some(Limit::Edit), "{cmd}: {d:?}");
    }
    // A push onto the protected target waits at the target limit.
    let d = fx.decide(
        ChatMode::Auto,
        "run",
        "git push origin HEAD:main",
        Some(&target),
    );
    assert_eq!(held_at(&d), Some(Limit::Target), "{d:?}");
    // A control file written: refused, never allowed.
    for cmd in [
        "echo '{}' > .claude/settings.local.json",
        "cp x .fallow/squad.toml",
        "rm -rf .git/hooks",
    ] {
        match fx.decide(ChatMode::Auto, "run", cmd, Some(&target)) {
            Decision::Refuse {
                limit: Limit::Control,
                ..
            } => {}
            other => panic!("{cmd}: {other:?}"),
        }
    }
    // An edit in the engineer's folder, or in the home folder outside the temp folder, waits;
    // one in the temp folder is scratch space and runs.
    for path in [
        format!("{f}/src/a.py"),
        "~/notes.md".to_string(),
        "/usr/local/share/fallow-notes.md".to_string(),
    ] {
        let d = fx.decide(ChatMode::Auto, "edit", &path, None);
        assert_eq!(held_at(&d), Some(Limit::Edit), "{path}: {d:?}");
    }
    let tmp = std::env::temp_dir().join("fallow-scratch/result.md");
    for path in [
        "/tmp/cropscan/result1.md".to_string(),
        "/private/tmp/x/y.md".to_string(),
        tmp.display().to_string(),
    ] {
        assert_eq!(
            fx.decide(ChatMode::Auto, "edit", &path, None),
            Decision::Allow,
            "{path}"
        );
    }
    // A control file edited: refused, even in the temp folder's spelling of the copy.
    match fx.decide(
        ChatMode::Auto,
        "edit",
        &wt.join(".claude/settings.json").display().to_string(),
        None,
    ) {
        Decision::Refuse {
            limit: Limit::Control,
            ..
        } => {}
        other => panic!("{other:?}"),
    }

    // 3. The same requests through a real session in Auto: no card, no refusal. The scratch
    // folder is this test's own (`/tmp/cropscan` is the engineer's), the commands otherwise
    // as written.
    let scan = root.join("cropscan");
    std::fs::create_dir_all(&scan).unwrap();
    for n in 2..=6 {
        let rows: String = (1..=40)
            .map(|i| format!("Plants of Rubus {i}\tcrop\tx_ast.yml\treq {i}\tnote\n"))
            .collect();
        std::fs::write(
            scan.join(format!("shard{n}.tsv")),
            format!("crop\tdoc\tf\tr\tn\n{rows}"),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, assistant_step("hello", "")).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(
        tx,
        Req::AddProject {
            path: folder.clone(),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&folder, &["add", "."]);
    git(&folder, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "get acquainted");
    let v = wait_turn(tx, &s, 1);
    let copy = PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    std::fs::create_dir_all(copy.join("agentic-workflow/classifier/output")).unwrap();
    let scan_text = scan.display().to_string();
    let as_here = |w: &str| {
        w.replace("{WT}", &copy.display().to_string())
            .replace("/tmp/cropscan", &scan_text)
    };
    let exec: Vec<String> = TEN
        .iter()
        .filter(|(_, k, _)| *k == "run")
        .map(|(_, _, w)| lit(&as_here(w)))
        .collect();
    let edits: Vec<String> = TEN
        .iter()
        .filter(|(_, k, _)| *k == "edit")
        .map(|(id, _, w)| format!("{{ path = {}, text = \"table {id}\" }}", lit(&as_here(w))))
        .collect();
    let mut text = std::fs::read_to_string(&scenario).unwrap();
    text.push_str(&assistant_step(
        "analysed",
        &format!(
            "exec = [{}]\nedit = [{}]",
            exec.join(", "),
            edits.join(", ")
        ),
    ));
    std::fs::write(&scenario, text).unwrap();
    say_solo(tx, &s, "do the exploratory analysis");
    let v = wait_turn(tx, &s, 2);
    let t = &v["turns"][1];
    assert_eq!(t["reply"], "analysed", "{t}");
    assert!(
        t["approvals"].as_array().unwrap().is_empty(),
        "no card: {t}"
    );
    assert!(
        t["refusals"].as_array().unwrap().is_empty(),
        "no refusal: {t}"
    );
    assert!(v["pending_approval"].is_null());
    for (n, id) in [(1, "r4"), (2, "r8"), (5, "r10")] {
        assert_eq!(
            std::fs::read_to_string(scan.join(format!("result{n}.md"))).unwrap(),
            format!("table {id}")
        );
    }
    // Nothing queued behind the turn: no turn of its own for any of them.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
    assert_eq!(v["turns"].as_array().unwrap().len(), 2, "{}", v["turns"]);
    stop(e);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&root);
}
