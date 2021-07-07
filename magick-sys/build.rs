use std::env;
use std::path::Path;
use std::process::{exit, Command};

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
        .arg("--disable-osx-universal-binary")
        .arg("--with-magick-plus-plus=no")
        .arg("--with-perl=no")
        .arg("--disable-dependency-tracking")
        .arg("--disable-silent-rules")
        .arg("--disable-opencl")
        .arg("--with-freetype=yes")
        .arg("--with-modules")
        .arg("--with-openjp2")
        .arg("--with-openexr")
        .arg("--with-webp=yes")
        .arg("--with-heic=no")
        .arg("--with-gslib")
        .arg("--without-fftw")
        .arg("--without-pango")
        .arg("--without-x")
        .arg("--without-wmf")
        .arg("--prefix");

    if cfg!(feature = "static") {
        configure_cmd.arg("--disable-shared").arg("--enable-static");
    }

    configure_cmd.arg(&out_dir);

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

    let previous_value = env::var("PKG_CONFIG_PATH").unwrap();

    env::set_var("PKG_CONFIG_PATH", format!("{}/lib/pkgconfig", &out_dir));
    let lib = pkg_config::Config::new()
        .cargo_metadata(true)
        .statik(cfg!(feature = "static"))
        .probe("MagickWand")?;

    // restore env
    env::set_var("PKG_CONFIG_PATH", previous_value);

    for d in lib.include_paths {
        println!("cargo:include={}", d.to_string_lossy());
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
