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
        while let Some(arg) = args.next() {
            if arg == "--out-dir" {
                t.out_dir = PathBuf::from(args.next().expect("missing out dir"));
            }
            if arg == "--crate-name" {
                t.crate_name = args.next().expect("missing crate name");
            }
        }
        if !t.out_dir.is_dir() {
            panic!("could not find out_dir, or not a directory")
        }
        if t.crate_name.is_empty() {
            panic!("could not find crate name")
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
        if let Some((k, v)) = buf.split_once('=') {
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
