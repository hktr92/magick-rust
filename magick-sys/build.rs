use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

static HEADER: &'static str = "#include <MagickWand/MagickWand.h>\n";

fn main() -> Result<(), Error> {
    if cfg!(not(target_os = "linux")) {
        eprintln!("magick-sys can't be compiled on win32 at this time. sorry.");
        exit(1);
    }

    build_imagemagick()?;

    Ok(())
}

fn build_imagemagick() -> Result<(), Error> {
    let magick_cfg = Magick {
        dir: "ImageMagick".into(),
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let num_jobs = env::var("NUM_JOBS");

    let cmd_path = std::fs::canonicalize(format!("{}/configure", &magick_cfg.dir)).unwrap();

    let mut configure_cmd = Command::new(cmd_path);

    configure_cmd
        .current_dir(&magick_cfg.path())
        // .arg("--disable-osx-universal-binary")
        .arg("--with-magick-plus-plus=no")
        .arg("--with-perl=no")
        // .arg("--disable-dependency-tracking")
        // .arg("--disable-silent-rules")
        // .arg("--disable-opencl");
        // .arg("--with-freetype=yes")
        .arg("--with-modules");
    // .arg("--with-openjp2")
    // .arg("--with-openexr")
    // .arg("--with-webp=yes")
    // .arg("--with-heic=no")
    // .arg("--with-gslib")
    // .arg("--without-fftw")
    // .arg("--without-pango")
    // .arg("--without-x")
    // .arg("--without-wmf");

    if cfg!(feature = "static") {
        configure_cmd.arg("--disable-shared").arg("--enable-static");
    }

    configure_cmd.arg("--prefix").arg(&out_dir);

    match configure_cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                eprintln!(
                    "`configure` failed:\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8(out.stdout).unwrap(),
                    String::from_utf8(out.stderr).unwrap()
                );
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("`configure` command execution failed: {:?}", e);
            exit(1)
        }
    }

    eprintln!("running `make install`...");
    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(&magick_cfg.path()).arg("install");

    if let Ok(jobs) = num_jobs {
        make_cmd.arg(format!("-j{}", jobs));
    }

    match make_cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                eprintln!(
                    "`make install` failed:\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8(out.stdout).unwrap(),
                    String::from_utf8(out.stderr).unwrap()
                );
                exit(1)
            }
        }
        Err(e) => {
            eprintln!("`make install` command execution failed: {:?}", e);
            exit(1)
        }
    }
    eprintln!("finished `make install`");

    let previous_value = env::var("PKG_CONFIG_PATH");

    env::set_var("PKG_CONFIG_PATH", format!("{}/lib/pkgconfig", &out_dir));
    let lib = pkg_config::Config::new()
        .cargo_metadata(true)
        .statik(cfg!(feature = "static"))
        .probe("MagickWand")?;

    // restore env
    if let Ok(previous_value) = previous_value {
        env::set_var("PKG_CONFIG_PATH", previous_value);
    }

    for lib in &lib.libs {
        let kind = if cfg!(feature = "static") {
            "static"
        } else {
            "dylib"
        };
        println!("cargo:rustc-link-lib={}={}", kind, lib);
    }

    for d in &lib.include_paths {
        println!("cargo:include={}", d.to_string_lossy());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_path_str = out_dir.join("bindings.rs");

    #[derive(Debug)]
    struct IgnoreMacros(HashSet<String>);

    impl bindgen::callbacks::ParseCallbacks for IgnoreMacros {
        fn will_parse_macro(&self, name: &str) -> bindgen::callbacks::MacroParsingBehavior {
            if self.0.contains(name) {
                bindgen::callbacks::MacroParsingBehavior::Ignore
            } else {
                bindgen::callbacks::MacroParsingBehavior::Default
            }
        }
    }

    let ignored_macros = IgnoreMacros(
        vec![
            "FP_INFINITE".into(),
            "FP_NAN".into(),
            "FP_NORMAL".into(),
            "FP_SUBNORMAL".into(),
            "FP_ZERO".into(),
            "IPPORT_RESERVED".into(),
            "FP_INT_UPWARD".into(),
            "FP_INT_DOWNWARD".into(),
            "FP_INT_TOWARDZERO".into(),
            "FP_INT_TONEARESTFROMZERO".into(),
            "FP_INT_TONEAREST".into(),
        ]
        .into_iter()
        .collect(),
    );

    if !Path::new(&bindings_path_str).exists() {
        // Create the header file that rust-bindgen needs as input.
        let gen_h_path = out_dir.join("gen.h");
        let mut gen_h = File::create(&gen_h_path).expect("could not create file");
        gen_h
            .write_all(HEADER.as_bytes())
            .expect("could not write header file");

        // Geneate the bindings.
        let mut builder = bindgen::Builder::default()
            .emit_builtins()
            .ctypes_prefix("libc")
            .raw_line("extern crate libc;")
            .header(gen_h_path.to_str().unwrap())
            .size_t_is_usize(true)
            .parse_callbacks(Box::new(ignored_macros))
            .blocklist_type("timex")
            .blocklist_function("clock_adjtime");

        for d in &lib.include_paths {
            builder = builder.clang_arg(format!("-I{}", d.to_string_lossy()));
        }

        let bindings = builder.generate().unwrap();
        // let mut file = File::create(&bindings_path_str).expect("could not create bindings file");
        // Work around the include! issue in rustc (as described in the
        // rust-bindgen README file) by wrapping the generated code in a
        // `pub mod` declaration; see issue #359 in (old) rust-bindgen.
        // file.write(b"pub mod bindings {\n").unwrap();
        // file.write(bindings.to_string().as_bytes()).unwrap();
        // file.write(b"\n}").unwrap();

        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("Couldn't write bindings!");

        std::fs::remove_file(&gen_h_path).expect("could not remove header file");
    }

    Ok(())
}

#[derive(Debug)]
enum Error {
    Wrapped(Box<dyn std::error::Error>),
}

impl std::convert::From<pkg_config::Error> for Error {
    fn from(e: pkg_config::Error) -> Self {
        Error::Wrapped(Box::new(e))
    }
}

impl std::convert::From<std::env::VarError> for Error {
    fn from(e: std::env::VarError) -> Self {
        Error::Wrapped(Box::new(e))
    }
}

#[derive(Debug)]
struct Magick {
    dir: String,
}

impl Magick {
    fn path(&self) -> &Path {
        &Path::new(&self.dir)
    }
}
