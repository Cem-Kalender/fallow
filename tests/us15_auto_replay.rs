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

/// Three more the same chat refused later: two the same way, and r13, a quoted here-document
/// writing a script inside the session's copy, whose body named nothing.
const LATER: &[(&str, &str, &str)] = &[
    (
        "r13",
        "run",
        r####"cat > _cropscan/write.py <<'EOF'
import json, csv, collections, glob
D = "agentic-workflow/classifier/output/"
cand = {r["id"]: r for r in csv.DictReader(open("_cropscan/candidates.tsv", encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE)}
out = {}
for f in sorted(glob.glob("_cropscan/out*.tsv")):
    for r in csv.DictReader(open(f, encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE): out[r["id"]] = r
occ = collections.defaultdict(list); total = 0
for l in open("_cropscan/all.jsonl", encoding="utf-8"):
    r = json.loads(l); occ[r["crop"].replace("\t", " ⇥ ")].append(r); total += 1
ORDER = ["REQ", "COND", "REF", "ATTR", "ODD"]
rows = []
for i, f in out.items():
    c = cand[i]["crop"]
    for o in occ[c]:
        rows.append([f["category"], o["doc"].split("/")[0], o["doc"], o["path"], c, f["offending_part"], f["belongs_in"], f["already_elsewhere"], f["note"], i])
rows.sort(key=lambda r: (ORDER.index(r[0]), r[1], r[2], r[3]))
with open(D + "crops_audit.tsv", "w", encoding="utf-8", newline="") as fh:
    w = csv.writer(fh, delimiter="\t", lineterminator="\n", quoting=csv.QUOTE_MINIMAL)
    w.writerow(["category", "sector", "doc", "path", "crop", "offending_part", "belongs_in", "already_elsewhere", "note", "string_id"])
    w.writerows(rows)
secs = sorted({r[1] for r in rows})
dist = collections.Counter(r[0] for r in out.values() for _ in [0])
dist = collections.Counter(out[i]["category"] for i in out)
occc = collections.Counter(r[0] for r in rows)
docs = collections.defaultdict(set)
for r in rows: docs[r[0]].add(r[2])
by = collections.Counter((r[0], r[1]) for r in rows)
topdocs = collections.Counter(r[2] for r in rows).most_common(12)
L = []
L.append("# Audit: wat in `crops` staat en er niet hoort (2026-11-24)\n")
L.append("Verkennende analyse, geen besluit en geen herstel. Elke vindplaats staat in `crops_audit.tsv` (één regel per voorkomen, met document en pad).\n")
L.append("## Werkwijze\n")
L.append(f"- Alle `crops`-waarden uit de {len({o['doc'] for v in occ.values() for o in v})} `_ast.yml` onder `classifier/output/`, op elke diepte: {total} voorkomens, {len(occ)} verschillende strings.")
L.append(f"- Kandidaten: {len(cand)} strings die lang zijn (>30 tekens of >3 woorden), leestekens of cijfers bevatten, met een kleine letter beginnen of een trefwoord bevatten (vrij, verbod, vereist, origine, product, materiaal, alleen, zie, …). De overige {len(occ)-len(cand)} zijn korte namen zonder leesteken of trefwoord (steekproef van 80: alle kale taxon- of volksnamen) en zijn niet beoordeeld.")
L.append("- Zeven subagents beoordeelden de kandidaten tegen `extractie/regels/veld-crops.md` en E22, E30 en E37: wat die regels toestaan (letterlijke productlabels, waardplantengroepen met hun organisme, restgroepen, synoniemen, `(zie register …)`, `(Alleen …)`) is niet gemarkeerd. Twijfelgevallen en de meeste REQ/COND zijn in het brondocument nagekeken; de rest is op de string beoordeeld (`already_elsewhere: unchecked`).\n")
L.append("## Categorieën\n")
L.append("| Categorie | Betekenis | Strings | Voorkomens | Documenten |\n|---|---|---|---|---|")
mean = {"REQ": "een eis, verbod of vergunningsuitspraak in de crop", "COND": "een voorwaarde: herkomst, gebied, plaagstatus, datum of periode", "REF": "een verwijzing, kop of voetnootteken in plaats van of aan het product", "ATTR": "een eigenschap (`attributes`), vorm (`productForm`) of uitzondering (`exclusion`) in de crop", "ODD": "anders fout: samengevoegde namen, verpakking als crop, tabs, fragmenten"}
for k in ORDER: L.append(f"| {k} | {mean[k]} | {dist[k]} | {occc[k]} | {len(docs[k])} |")
L.append(f"| totaal | | {len(out)} | {len(rows)} | {len({r[2] for r in rows})} |\n")
L.append("## Per sector (voorkomens)\n")
L.append("| Sector | " + " | ".join(ORDER) + " |\n|---|" + "---|" * len(ORDER))
for s in secs: L.append(f"| {s} | " + " | ".join(str(by[(k, s)]) for k in ORDER) + " |")
L.append("\n## Documenten met de meeste vindplaatsen\n")
L.append("| Document | Vindplaatsen |\n|---|---|")
for d, n in topdocs: L.append(f"| {d} | {n} |")
for k in ["REQ", "COND"]:
    L.append(f"\n## Alle {k}\n")
    L.append("| Document | Pad | Crop | Fout deel | Hoort in | Al elders |\n|---|---|---|---|---|---|")
    for r in rows:
        if r[0] == k:
            L.append(f"| {r[2].replace('_ast.yml','')} | `{r[3]}` | {r[4][:160].replace('|','/')} | {r[5][:90].replace('|','/')} | {r[6]} | {r[7]} |")
L.append("\n## Patronen\n")
L.append("- **REF is grotendeels voetnoottekens** (`*`, `**`, een cijfer) aan een taxonnaam geplakt (\"Alpinia spp.**\", \"Buxus6\", \"Rubus idaeus3\"), vooral in SIER (Zuid-Korea, Martinique, Turkije, Puerto Rico, Zuid-Afrika, Taiwan). Daarnaast koppen als crop (\"De inspectieplichtige producten\", \"Alle toegelaten producten\") en verwijzingen naar externe lijsten.")
L.append("- **COND** komt uit twee bronnen: de EU-volzinnen van SIER Turkije 2.43 (\"Plants of X … originating in countries where … is known to occur\") en de vergunningslijst van ZZ Nieuw-Zeeland 10.4 (\"Pinus (indien uit productiegebied niet bekend als vrij van Fusarium circinatum)\"); verder \"(origine NL)\", \"(geldt alleen voor Florida …)\" en ingangsdatums in GF.")
L.append("- **ATTR**: gebruik en toestand (\"voor consumptie\", \"gedroogde\", \"ongepelde\") vooral in DIV; maat, beworteling en stek, \"m.u.v.\"-lijsten en in-vitro-zinnen vooral in SIER. Vaak staat de vorm of de uitzondering al in `productForm` of `exclusion` en is de tekst in de crop een dubbeling.")
L.append("- **ODD**: verpakking als crop (zakken, dozen, pallets, fust), samengevoegde goederen (\"Aubergine & Tomaat\", \"Paprika/Peper\", \"Hybriden van Cydonia, Malus, Prunus en Pyrus\"), losse fragmenten (\"pit\", \"divaricatum\") en tabs.")
L.append("\n## Voorbehoud\n")
L.append("- De categorie is een oordeel van een subagent; REQ en de meeste COND zijn nagekeken, ATTR/REF/ODD grotendeels alleen op de string. Er zitten grensgevallen in: toestandswoorden die deel van de productnaam kunnen zijn (\"Gedroogde vijgen\"), \"GECONDITIONEERDE TEELT\" (mogelijk een toegestane kop), \"o.a.\"-opsommingen.")
L.append("- Elke string telt in één categorie, de ernstigste (REQ > COND > REF > ATTR > ODD); de `note` noemt de andere.")
L.append("- \"(alleen voor PEQ)\" en andere \"(Alleen …)\"-toevoegingen zijn als toegestaan gelaten omdat `veld-crops.md` ze bewaart.")
open(D + "crops_audit.md", "w", encoding="utf-8").write("\n".join(L) + "\n")
print(len(rows), dist, occc)
EOF
uv run --no-project python -I _cropscan/write.py && sed -n 1,60p agentic-workflow/classifier/output/crops_audit.md"####,
    ),
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

    // 1. All ten, and the three after them, as written: allowed in Auto, no card.
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
        "cat > .fallow/agents/x.toml <<'EOF'\nx = 1\nEOF",
        "cat <<EOF > .claude/settings.json\n{}\nEOF",
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
