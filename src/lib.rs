use std::io::Read;
use std::{env, fs, path, process};

/// A scoped wrapper for the directory where we'll compile and run the build script.
struct BuildDir {
    pub path: path::PathBuf,
}

impl BuildDir {
    fn new() -> Self {
        let mut random_data = [0u8; 16];
        let mut file = fs::File::open("/dev/urandom").expect("failed to open /dev/urandom");
        file.read_exact(&mut random_data)
            .expect("failed to read /dev/urandom");

        let mut hex_str = String::new();
        for digit in random_data.iter() {
            hex_str = hex_str + &format!("{:x}", digit)
        }

        BuildDir {
            path: format!("/tmp/build-script-{}", hex_str).into(),
        }
    }
}

impl Drop for BuildDir {
    fn drop(&mut self) {
        // some paranoia before running 'rm -rf'
        assert!(self.path.starts_with("/tmp"));

        println!("Removing build crate staging dir: {}", self.path.display());
        fs::remove_dir_all(&self.path).expect(&format!(
            "Couldn't clean up build dir: {}",
            self.path.display()
        ));
    }
}

fn cp_r(in_dir: &path::Path, out_dir: &path::Path) {
    let res = process::Command::new("cp")
        .arg("-r")
        .arg(in_dir)
        .arg(out_dir)
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .output()
        .expect(&format!(
            "Failed to cp -r {} {}",
            in_dir.display(),
            out_dir.display()
        ));

    assert!(
        res.status.success(),
        "Failed to cp -r {} {} with {:?}",
        in_dir.display(),
        out_dir.display(),
        res
    );
}


fn qualify_cargo_toml_paths_in_text(cargo_toml_content: &str, base_dir: &path::Path) -> String {
    // This is completely manual to avoid introducing any dependencies in this
    // library, since the whole point is to work around dependency issues.

    // Lacking a real parser due to constraints, look for a couple of common
    // patterns. TODO: Roll a little parser for this.
    let mut cargo_toml = cargo_toml_content.to_owned();
    cargo_toml = cargo_toml.replace("path = \"", &format!("path = \"{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path=\"", &format!("path=\"{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path = '", &format!("path = '{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path='", &format!("path='{}/", base_dir.display()));
    cargo_toml
}

fn qualify_cargo_toml_paths(cargo_toml_path: &path::Path, base_dir: &path::Path) {
    let cargo_toml = fs::read_to_string(cargo_toml_path).unwrap();
    let cargo_toml = qualify_cargo_toml_paths_in_text(&cargo_toml, &base_dir);

    fs::write(cargo_toml_path, cargo_toml).expect(&format!(
        "Failed to write modified Cargo.toml at {}",
        cargo_toml_path.display()
    ));
}

fn compile_build_crate(build_dir: &BuildDir, cargo: &str, path: &str) {
    let res = process::Command::new(cargo)
        .args(&["build", "-vv"])
        .env_clear()
        .env("PATH", path)
        .current_dir(&build_dir.path)
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .output()
        .expect("failed to compile build-script crate");

    assert!(
        res.status.success(),
        "Failed to run compile build crate at {} with {:#?}",
        build_dir.path.display(),
        res
    );
}

fn run_build_script(build_dir: &BuildDir, executable_name: &str, working_dir: &path::Path) {
    // run the build script
    let build_script_path = build_dir
        .path
        .join("target")
        .join("debug")
        .join(executable_name);

    let res = process::Command::new(&build_script_path)
        .current_dir(&working_dir)
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .output()
        .expect(&format!(
            "failed to run build script at {}",
            build_script_path.display()
        ));

    assert!(
        res.status.success(),
        "Failed to run build script at {} with {:#?}",
        build_script_path.display(),
        res
    );
}

pub fn run_build_crate<P: AsRef<path::Path>>(build_crate_src: P) {
    let build_crate_src = build_crate_src.as_ref();
    println!("cargo:rerun-if-changed={}", build_crate_src.display());

    let build_dir = BuildDir::new();

    let executable_name = build_crate_src
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .expect(&format!(
            "Couldn't get file name from build crate src dir: {}",
            build_crate_src.display(),
        ));

    let cargo = env::var("CARGO").unwrap();
    let path = env::var("PATH").unwrap();
    let base_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let base_dir = path::Path::new(&base_dir).join("build-script");

    // Copy the build crate into /tmp to avoid the influence of .cargo/config
    // settings in the build crate's parent, which cargo gives us no way to
    // ignore.
    println!(
        "Copying build crate source from {} to {}",
        &build_crate_src.display(),
        build_dir.path.display()
    );
    cp_r(build_crate_src, &build_dir.path);

    // Having copied the crate, we need to fix any relative paths that were in
    // the Cargo.toml
    qualify_cargo_toml_paths(&build_dir.path.join("Cargo.toml"), &base_dir);

    compile_build_crate(&build_dir, &cargo, &path);

    // Run the build script with its original source directory as the working
    // dir.
    run_build_script(&build_dir, &executable_name, &build_crate_src);
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_path_fixup_1() {
        let input = r#"
[dependencies]
lib-crate = { path = "../../lib-crate" }
"#;
        let expected = r#"
[dependencies]
lib-crate = { path = "/basedir/../../lib-crate" }
"#;

        assert_eq!(qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
                   expected.to_string());
    }

    #[test]
    fn test_path_fixup_2() {
        let input = r#"
[dependencies]
lib-crate = { path="../../lib-crate" }
"#;
        let expected = r#"
[dependencies]
lib-crate = { path="/basedir/../../lib-crate" }
"#;

        assert_eq!(qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
                   expected.to_string());
    }

    #[test]
    fn test_path_fixup_3() {
        let input = r#"
[dependencies]
lib-crate = { path = '../../lib-crate' }
"#;
        let expected = r#"
[dependencies]
lib-crate = { path = '/basedir/../../lib-crate' }
"#;

        assert_eq!(qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
                   expected.to_string());
    }

    #[test]
    fn test_path_fixup_4() {
        let input = r#"
[dependencies]
lib-crate = { path='../../lib-crate' }
"#;
        let expected = r#"
[dependencies]
lib-crate = { path='/basedir/../../lib-crate' }
"#;

        assert_eq!(qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
                   expected.to_string());
    }

}
