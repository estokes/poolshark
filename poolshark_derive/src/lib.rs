use proc_macro::{Span, TokenStream};
use quote::quote;
use std::{
    cmp::max,
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Seek, Write},
    path::{Path, PathBuf},
};

struct BuildEnv {
    out_dir: PathBuf,
    crate_name: String,
}

impl BuildEnv {
    fn get() -> Self {
        let mut t = Self {
            out_dir: PathBuf::new(),
            crate_name: String::new(),
        };
        let mut args = std::env::args();
        let mut dep = PathBuf::new();
        while let Some(arg) = args.next() {
            if arg == "--out-dir" {
                t.out_dir = PathBuf::from(args.next().expect("missing out dir"));
            }
            if arg.starts_with("--out-dir=") {
                t.out_dir = PathBuf::from(
                    arg.strip_prefix("--out-dir=").expect("invalid out-dir"),
                );
            }
            if arg == "--crate-name" {
                t.crate_name = args.next().expect("missing crate name");
            }
            if arg.starts_with("--crate-name=") {
                t.crate_name = arg
                    .strip_prefix("--crate-name=")
                    .expect("invalid crate-name")
                    .to_owned();
            }
            if let Some(s) = arg.strip_prefix("dependency=") {
                dep = PathBuf::from(s)
            }
        }
        if !t.out_dir.is_dir() {
            if dep.is_dir() {
                t.out_dir = dep;
            } else {
                t.out_dir = PathBuf::from("target/debug/deps");
                if !t.out_dir.is_dir() {
                    panic!("could not find out dir")
                }
            }
        }
        if t.crate_name.is_empty() || t.crate_name.starts_with("-") {
            match std::env::var("CARGO_PKG_NAME") {
                Ok(n) => t.crate_name = n,
                Err(_) => {
                    let dir = t.out_dir.parent().expect("could not find crate name");
                    let dir = dir.parent().expect("could not find crate name");
                    let dir = dir.parent().expect("could not find crate name");
                    t.crate_name = String::from_utf8_lossy(
                        dir.file_name()
                            .expect("could not find crate name")
                            .as_encoded_bytes(),
                    )
                    .into_owned();
                }
            }
        }
        t
    }
}

fn allocate_id(path: &Path, key: String) -> u16 {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .expect("could not open location ids file");
    file.lock().expect("could not lock file");
    let mut reader = BufReader::new(file);
    let mut ids = BTreeMap::new();
    let mut max_id = 0;
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(n) => {
                if n == 0 {
                    break;
                }
            }
            Err(e) => panic!("failed to read line {e}"),
        }
        if let Some((k, v)) = buf.rsplit_once('=') {
            let id: u16 = v.trim().parse().expect("invalid id");
            max_id = max(max_id, id);
            ids.insert(k.trim().to_string(), id);
        }
    }
    match ids.get(&key) {
        Some(id) => *id,
        None => {
            let id = max_id + 1;
            if id < max_id {
                panic!("too many poolshark location ids")
            }
            ids.insert(key, id);
            let mut file = reader.into_inner();
            file.seek(io::SeekFrom::Start(0))
                .expect("could not seek to beginning");
            for (k, v) in ids {
                write!(file, "{k} = {v}\n").expect("could not write line")
            }
            file.sync_all().expect("could not sync data");
            id
        }
    }
}

/// Generate a globally unique identifier for a source code position
///
/// Every time you invoke this macro it will return generate a unique LocationId
/// that corresponds to the position in the source code where it was invoked.
/// This works even across crates, so even if two crates in your project invoke
/// this macro on the exact same line and column of the exact same file the ID
/// will still be unique.
///
/// ### How?
///
/// The ids are stored in a file in the compiler's OUT_DIR, where it's
/// outputting all the object code for every crate. The file contains a btreemap
/// of ids where the key is crate_name::file_path::line_number::column_number
/// and the value is the id.
///
/// ### Failure Modes
///
/// - Your build system is so exceedingly strange that somehow you are calling
/// rustc without passing the crate name or the out dir through any known
/// mechanism (several are tried.). In this case the macro may produce a compile error.
///
/// - You are dynamicaly loading separately compiled code. In this case the ids
/// may conflict, however in our use case (poolshark) we don't care about this
/// case because separately compiled code will have separate thread local
/// structures and thus poolshark will still work fine
#[proc_macro]
pub fn location_id(_input: TokenStream) -> TokenStream {
    let cfg = BuildEnv::get();
    let loc = Span::call_site();
    let key = format!(
        "{}:{}:{}:{}",
        cfg.crate_name,
        loc.file(),
        loc.line(),
        loc.column()
    );
    let path = cfg.out_dir.join(".poolshark_loc_ids");
    let id = allocate_id(&path, key);
    if cfg.crate_name == "poolshark" {
        quote!(crate::LocationId(#id)).into()
    } else {
        quote!(poolshark::LocationId(#id)).into()
    }
}
