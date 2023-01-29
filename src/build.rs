use ructe::Ructe;
use std::{fs::File, io::Read, path::Path, process::Command};

fn git_info() {
    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        if output.status.success() {
            let git_hash = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=GIT_HASH={git_hash}");
            println!("cargo:rustc-env=GIT_SHORT_HASH={}", &git_hash[..8])
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
    {
        if output.status.success() {
            let git_branch = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=GIT_BRANCH={git_branch}");
        }
    }
}

fn version_info() -> Result<(), anyhow::Error> {
    let cargo_toml = Path::new(&std::env::var("CARGO_MANIFEST_DIR")?).join("Cargo.toml");

    let mut file = File::open(cargo_toml)?;

    let mut cargo_data = String::new();
    file.read_to_string(&mut cargo_data)?;

    let data: toml::Value = toml::from_str(&cargo_data)?;

    if let Some(version) = data["package"]["version"].as_str() {
        println!("cargo:rustc-env=PKG_VERSION={version}");
    }

    if let Some(name) = data["package"]["name"].as_str() {
        println!("cargo:rustc-env=PKG_NAME={name}");
    }

    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    git_info();
    version_info()?;

    let mut ructe = Ructe::from_env()?;
    let mut statics = ructe.statics()?;
    statics.add_sass_file("scss/index.scss")?;
    ructe.compile_templates("templates")?;

    Ok(())
}
