use proc_macro::{Span, TokenStream};
use quote::quote;
use std::{
    cmp::max,
    collections::BTreeMap,
    env,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Seek, Write},
    path::Path,
};

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
    let crate_name = env::var("CARGO_PKG_NAME").expect("missing CARGO_PKG_NAME");
    let loc = Span::call_site();
    let key = format!(
        "{}:{}:{}:{}",
        crate_name,
        loc.file(),
        loc.line(),
        loc.column()
    );
    let path = Path::new(".poolshark_loc_ids");
    let id = allocate_id(path, key);
    quote!(#id).into()
}
