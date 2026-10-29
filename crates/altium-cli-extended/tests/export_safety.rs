mod common;
mod support;
use common::*;

#[test]
fn exports_refuse_design_paths_even_in_dry_run() {
    let dir = tempdir("export-guard");
    let lib = dir.join("L.PcbLib");
    let pcb = dir.join("B.PcbDoc");
    let sch = dir.join("S.SchDoc");
    support::pcblib(&lib);
    support::pcb(&pcb);
    support::sch(&sch, &["victim.PcbDoc"]);
    let victim = dir.join("victim.PcbDoc");
    std::fs::write(&victim, b"do not overwrite").unwrap();
    for dry in [false, true] {
        let l = s(&lib); let p = s(&pcb); let sh = s(&sch); let d = s(&dir);
        for mut args in [
            vec!["pcblib", "svg", &l, "A", "--out", &l],
            vec!["pcb", "svg", &p, "--out", &p],
            vec!["raw", "dump", &l, "Library/Data", "--out", &l],
            vec!["pcblib", "models", &l, "--extract", &d],
            vec!["pcb", "models", &p, "--extract", &d],
            vec!["sch", "images", &sh, "--extract", &d],
        ] {
            let before: Vec<_> = [&lib, &pcb, &sch, &victim].iter().map(|p| std::fs::read(p).unwrap()).collect();
            if dry { args.push("--dry-run"); }
            err(&args, "UNSAFE");
            let after: Vec<_> = [&lib, &pcb, &sch, &victim].iter().map(|p| std::fs::read(p).unwrap()).collect();
            assert_eq!(before, after, "{args:?}");
        }
    }
    let out = dir.join("new/nested/preview.svg");
    ok(&["pcblib", "svg", &s(&lib), "A", "--out", &s(&out)]);
    assert!(std::fs::read_to_string(out).unwrap().starts_with("<svg"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn image_names_stay_inside_output_and_avoid_windows_devices() {
    let dir = tempdir("image-names");
    let file = dir.join("S.SchDoc");
    support::sch(&file, &["C:evil.png", "CON.png", "NUL", "COM1.step", "LPT9.txt", "name. ", "..", "a.png", "A.PNG"]);
    let out = dir.join("images");
    let data = ok(&["sch", "images", &s(&file), "--extract", &s(&out)]);
    let paths = data["extracted"].as_array().unwrap();
    assert_eq!(paths.len(), 9);
    let mut names = std::collections::HashSet::new();
    for path in paths {
        let p = std::path::Path::new(path.as_str().unwrap());
        assert_eq!(p.parent(), Some(out.as_path()));
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains(':') && !name.ends_with(['.', ' ']));
        assert!(!["CON", "NUL", "COM1", "LPT9"].contains(&name.split('.').next().unwrap().to_ascii_uppercase().as_str()), "{name}");
        assert!(names.insert(name.to_ascii_lowercase()));
        assert_eq!(std::fs::read(p).unwrap(), b"image");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
