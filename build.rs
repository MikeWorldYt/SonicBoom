use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_WEB_UI").is_none() {
        return;
    }
    for path in [
        "frontend/src",
        "frontend/scripts",
        "frontend/index.html",
        "frontend/vite.config.ts",
        "frontend/tsconfig.json",
        "frontend/package.json",
        "frontend/package-lock.json",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=SONICBOOM_FRONTEND_PREBUILT");
    let prebuilt = env::var("SONICBOOM_FRONTEND_PREBUILT").as_deref() == Ok("1");
    if prebuilt {
        println!("cargo:rerun-if-changed=frontend/dist");
        assert!(
            Path::new("frontend/dist/index.html").is_file(),
            "Build the frontend before selecting SONICBOOM_FRONTEND_PREBUILT=1"
        );
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    if !prebuilt {
        let lock = fs::read("frontend/package-lock.json").unwrap();
        let stamp = Path::new("frontend/node_modules/.sonicboom-lock");
        if fs::read(stamp).ok().as_ref() != Some(&lock) {
            run(npm, &["ci"]);
            fs::write(stamp, lock).unwrap();
        }
    }
    if !prebuilt {
        run(npm, &["run", "build"]);
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::copy("frontend/dist/index.html", out.join("frontend.html")).unwrap();
    let mut entries = String::from(
        "fn asset_body(file: &str) -> Option<(&'static str, &'static [u8])> { match file {\n",
    );
    embed(Path::new("frontend/dist/assets"), &out, &mut entries);
    entries.push_str("_ => None, } }\n");
    fs::write(out.join("frontend_assets.rs"), entries).unwrap();
}
fn run(npm: &str, args: &[&str]) {
    let status = Command::new(npm)
        .args(args)
        .current_dir("frontend")
        .status()
        .expect(
            "Frontend build requires Node.js >=22.12 and npm. Install Node.js, then retry Cargo.",
        );
    assert!(
        status.success(),
        "Frontend build failed: npm {}",
        args.join(" ")
    );
}
fn embed(dir: &Path, out: &Path, entries: &mut String) {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            embed(&path, out, entries);
            continue;
        }
        let name = path
            .strip_prefix("frontend/dist/assets")
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let destination = out.join("assets").join(&name);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(&path, &destination).unwrap();
        let mime = match path.extension().and_then(|e| e.to_str()) {
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("svg") => "image/svg+xml",
            Some("woff2") => "font/woff2",
            _ => "application/octet-stream",
        };
        entries.push_str(&format!(
            "{name:?} => Some(({mime:?}, include_bytes!({destination:?}))),\n"
        ));
    }
}
