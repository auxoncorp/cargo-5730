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

        let mut dir = env::temp_dir();
        dir.push(format!("build-script-{}", hex_str));

        BuildDir { path: dir }
    }
}

impl Drop for BuildDir {
    fn drop(&mut self) {
        // some paranoia before running 'rm -rf'
        assert!(self.path.starts_with(env::temp_dir()));

        println!("Removing build crate staging dir: {}", self.path.display());
        fs::remove_dir_all(&self.path).expect(&format!(
            "Couldn't clean up build dir: {}",
            self.path.display()
        ));
    }
}

// All this toml stuff is completely manual to avoid introducing any
// dependencies in this library, since the whole point is to work around
// dependency issues.

fn qualify_cargo_toml_paths_in_text(cargo_toml_content: &str, base_dir: &path::Path) -> String {
    // Lacking a real parser due to constraints, look for a couple of common
    // patterns. TODO: Roll a little parser for this.
    let mut cargo_toml = cargo_toml_content.to_owned();
    cargo_toml = cargo_toml.replace("path = \"", &format!("path = \"{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path=\"", &format!("path=\"{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path = '", &format!("path = '{}/", base_dir.display()));
    cargo_toml = cargo_toml.replace("path='", &format!("path='{}/", base_dir.display()));
    cargo_toml
}

fn read_toml_section(toml_path: &path::Path, section_name: &str) -> String {
    let content = fs::read_to_string(toml_path)
        .expect(&format!("Can't read toml from {}", toml_path.display()));

    let mut section = String::new();
    let mut in_section = false;
    for line in content.lines() {
        if in_section {
            if line.starts_with("[") {
                break;
            } else {
                section.push_str(line);
                section.push_str("\n");
            }
        } else if line == format!("[{}]", section_name) {
            in_section = true
        }
    }

    section
}

fn compile_build_crate(
    build_dir: &BuildDir,
    cargo: &str,
    path: &str,
    ssh_auth_sock: &str,
    rustup_home: &str,
    rustup_toolchain: &str,
    build_crate_src: &path::Path,
) {
    let res = process::Command::new(cargo)
        .args(&["build", "-vv"])
        .env_clear()
        .env("PATH", path)
        .env("SSH_AUTH_SOCK", ssh_auth_sock)
        .env("RUSTUP_HOME", rustup_home)
        .env("RUSTUP_TOOLCHAIN", rustup_toolchain)
        .env("CARGO_TARGET_DIR", build_crate_src.join("build-script-target"))
        .env("RUSTFLAGS", "--cfg workaround_build")
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

fn run_compiled_build_script(executable_name: &str, working_dir: &path::Path) {
    // run the build script
    let build_script_path = working_dir
        .join("build-script-target")
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

/// Compile and run build.rs from cwd
/// - use 'workaround-build-dependencies' from Cargo.toml
/// - Build it with the workaround_build configuration
/// - Copy Cargo.lock back here to build.Cargo.lock
pub fn run_build_script() {
    let base_dir = env::var("CARGO_MANIFEST_DIR").expect("Can't get CARGO_MANIFEST_DIR from env");
    let base_dir = path::Path::new(&base_dir);

    let build_rs = base_dir.join("build.rs");
    let cargo_toml = base_dir.join("Cargo.toml");
    let cargo_build_lock = base_dir.join("Cargo.build.lock");

    let build_dir = BuildDir::new();

    let cargo = env::var("CARGO").expect("Can't get CARGO from env");
    let path = env::var("PATH").expect("Can't get PATH from env");
    let ssh_auth_sock = env::var("SSH_AUTH_SOCK").unwrap_or_default();

    let rustup_home = env::var("RUSTUP_HOME").unwrap_or_default();
    let rustup_toolchain = env::var("RUSTUP_TOOLCHAIN").unwrap_or_default();

    // Build in /tmp to avoid the influence of .cargo/config settings in the
    // build crate's parent, which cargo gives us no way to ignore.
    fs::create_dir(&build_dir.path).expect(&format!(
        "Couldn't create temp build dir at {}",
        build_dir.path.display()
    ));

    // copy in build.rs
    fs::create_dir(build_dir.path.join("src")).expect(&format!(
        "Couldn't create directory {}/src",
        build_dir.path.display()
    ));

    fs::copy(&build_rs, build_dir.path.join("src").join("main.rs")).expect(&format!(
        "Error copying build script {} to {}/src/main.rs",
        build_rs.display(),
        build_dir.path.display()
    ));

    // synthesize Cargo.toml
    let deps_section = read_toml_section(&cargo_toml, "workaround-build-dependencies");

    // Fix any relative paths that were in the Cargo.toml
    let deps_section = qualify_cargo_toml_paths_in_text(&deps_section, &base_dir);
    let cargo_toml_content = format!(
        r#"
[package]
name = "workaround-build-script"
version = "0.1.0"
authors = ["The cargo-5730 crate"]
edition = "2018"

[dependencies]
{}
"#,
        deps_section
    );

    fs::write(build_dir.path.join("Cargo.toml"), cargo_toml_content).expect(&format!(
        "Error writing synthesized Cargo manifest to {}/Cargo.toml",
        build_dir.path.display()
    ));

    // copy in Cargo.build.lock, if we have one
    if cargo_build_lock.exists() {
        fs::copy(&cargo_build_lock, build_dir.path.join("Cargo.lock")).expect(&format!(
            "Error copying build manifest lockfile from {} to {}/Cargo.lock",
            cargo_build_lock.display(),
            build_dir.path.display()
        ));
    }

    compile_build_crate(
        &build_dir,
        &cargo,
        &path,
        &ssh_auth_sock,
        &rustup_home,
        &rustup_toolchain,
        &base_dir,
    );

    // copy back the 'Cargo.lock' file, to speed up subsequent compilations
    fs::copy(build_dir.path.join("Cargo.lock"), &cargo_build_lock).expect(&format!(
        "Error copying out Cargo.lock from {}/Cargo.lock to {}",
        build_dir.path.display(),
        cargo_build_lock.display()
    ));

    // Run the build script with its original source directory as the working
    // dir.
    run_compiled_build_script("workaround-build-script", &base_dir);
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

        assert_eq!(
            qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
            expected.to_string()
        );
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

        assert_eq!(
            qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
            expected.to_string()
        );
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

        assert_eq!(
            qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
            expected.to_string()
        );
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

        assert_eq!(
            qualify_cargo_toml_paths_in_text(input, path::Path::new("/basedir")),
            expected.to_string()
        );
    }
}
