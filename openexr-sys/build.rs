extern crate cc;
extern crate pkg_config;
extern crate curl;
extern crate unzip;
extern crate cmake;

use std::path::{Path, PathBuf};
use std::env;
use std::fs::File;
use std::io::{Write};

use curl::easy::Easy;

fn main() {
    build_native_openexr();

    // Find and link OpenEXR and IlmBase
    let include_paths = {
        let mut include_paths = Vec::new();

        let suffix = if let Ok(v) = env::var("OPENEXR_LIB_SUFFIX") {
            format!("-{}", v)
        } else {
            "".into()
        };

        if let Ok(path) = env::var("OPENEXR_DIR") {
            // There's an environment variable, so let's use that
            println!("cargo:rustc-link-search=native={}/lib", path);
            println!("cargo:rustc-link-lib=static=IlmImf{}", suffix);
            println!("cargo:rustc-link-lib=static=IlmImfUtil{}", suffix);
            include_paths.push(PathBuf::from(&format!("{}/include/OpenEXR", path)));
        } else {
            // There's no enviroment variable, so use pkgconfig to find
            // the libs
            let paths = pkg_config::Config::new()
                .atleast_version("2.0.0")
                .probe("OpenEXR")
                .map(|openexr_cfg| openexr_cfg.include_paths.clone())
                .map_err(|err| {
                    panic!(
                        "couldn't find OpenEXR: environment variable \
                         OPENEXR_DIR is unset and pkg-config failed: {}",
                        err
                    )
                })
                .unwrap();

            include_paths.extend_from_slice(&paths);
        }

        if let Ok(path) = env::var("ILMBASE_DIR") {
            println!("cargo:rustc-link-search=native={}/lib", path);
            println!("cargo:rustc-link-lib=static=IexMath{}", suffix);
            println!("cargo:rustc-link-lib=static=Iex{}", suffix);
            println!("cargo:rustc-link-lib=static=Imath{}", suffix);
            println!("cargo:rustc-link-lib=static=IlmThread{}", suffix);
            println!("cargo:rustc-link-lib=static=Half{}", suffix);
            include_paths.push(PathBuf::from(&format!("{}/include/OpenEXR", path)));
        } else {
            let paths = pkg_config::Config::new()
                .atleast_version("2.0.0")
                .cargo_metadata(false) // OpenEXR already pulls in all the flags we need
                .probe("IlmBase")
                .map(|ilmbase_cfg| ilmbase_cfg.include_paths.clone())
                .map_err(|err| {
                    panic!(
                        "couldn't find IlmBase: environment variable \
                         ILMBASE_DIR is unset and pkg-config failed: {}",
                        err
                    )
                })
                .unwrap();
            include_paths.extend_from_slice(&paths);
        }

        include_paths
    };

    // Find and link zlib, needed for OpenEXR
    // Use environment variable if it exists, and otherwise use pkgconfig.
    if let Ok(path) = env::var("ZLIB_DIR") {
        println!("cargo:rustc-link-search=native={}/lib", path);
        println!("cargo:rustc-link-lib=static=zlibstatic");
    } else if let Err(err) = pkg_config::probe_library("zlib") {
        panic!(
            "couldn't find zlib: environment variable ZLIB_DIR is unset \
             and pkg-config failed: {}",
            err
        );
    }

    // Build C wrapper for OpenEXR
    let mut cc = cc::Build::new();
    cc.cpp(true).include("c_wrapper");
    #[cfg(target_env = "msvc")]
    cc.flag("/std:c++14");
    #[cfg(not(target_env = "msvc"))]
    cc.flag("-std=c++0x");
    for path in &include_paths {
        cc.include(path);
    }
    cc.file("c_wrapper/cexr.cpp")
        .file("c_wrapper/rust_istream.cpp")
        .file("c_wrapper/memory_istream.cpp")
        .file("c_wrapper/rust_ostream.cpp")
        .compile("libcexr.a");
    println!("cargo:rustc-link-search=native={}", Path::new(&env::var("OUT_DIR").unwrap()).join("build/openexr-2.4.1/build/install/bin").to_str().unwrap());
    println!("cargo:rustc-link-search=native={}", Path::new(&env::var("OUT_DIR").unwrap()).join("build/zlib-1.2.11/build/install/bin").to_str().unwrap());
}

fn build_native_openexr() {
    const ZLIB_DIR: &str = "zlib-1.2.11";
    const OPENEXR_DIR: &str = "openexr-2.4.1";

    let cur = env::current_dir().unwrap();
    let build_dir = Path::new(&env::var("OUT_DIR").unwrap()).join("build");
    std::fs::create_dir(build_dir.clone());
    env::set_current_dir(build_dir).unwrap();

    download(
        "https://github.com/AcademySoftwareFoundation/openexr/archive/v2.4.1.zip",
        "openexr.zip",
        "."
    ).unwrap();
    download(
        "https://www.zlib.net/zlib1211.zip",
        "zlib.zip",
        "."
    ).unwrap();
    cmake::Config::new(ZLIB_DIR)
    .out_dir(ZLIB_DIR)
    .define("CMAKE_INSTALL_PREFIX", "install")
    .profile("Release")
    .build();
    // this may fail in creating symbolic link on Windows
    std::panic::catch_unwind(|| {
        cmake::Config::new(OPENEXR_DIR)
        .out_dir(OPENEXR_DIR)
        .define("CMAKE_INSTALL_PREFIX", "install")
        .define("ZLIB_INCLUDE_DIR", format!("../../{}/build/install/include", ZLIB_DIR))
        .define("BUILD_TESTING", "OFF")
        .define("PYILMBASE_ENABLE", "OFF")
        .define("OPENEXR_VIEWERS_ENABLE", "OFF")
        .define("OPENEXR_BUILD_UTILS", "OFF")
        .profile("Release")
        .build();
    });
    env::set_var("OPENEXR_LIB_SUFFIX", "2_4");
    env::set_var("OPENEXR_DIR", env::current_dir().unwrap().join(OPENEXR_DIR).join("build/install"));
    env::set_var("ILMBASE_DIR", env::current_dir().unwrap().join(OPENEXR_DIR).join("build/install"));
    env::set_var("ZLIB_DIR", env::current_dir().unwrap().join(ZLIB_DIR).join("build/install"));

    env::set_current_dir(cur).unwrap();
}

fn download(url: &str, zipname: &str, dst: &str) -> Result<(), curl::Error> {
    if Path::new(zipname).exists() {
        return Ok(());
    }
    let mut file = File::create(zipname).unwrap();
    let mut easy = Easy::new();
    easy.url(url)?;
    easy.follow_location(true)?;
    easy.write_function(move |data| {
        let len = file.write(data).unwrap();
        file.sync_data().unwrap();
        Ok(len)
    })?;
    easy.perform()?;
    if let Err(code) = easy.response_code() {
        panic!("{}", code);
    }

    let mut file = File::open(zipname).unwrap();
    unzip::Unzipper::new(file, dst).unzip().unwrap();

    Ok(())
}
