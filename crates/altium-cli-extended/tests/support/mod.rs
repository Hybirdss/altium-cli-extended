#![allow(dead_code)]
use std::io::{Cursor, Write};
use std::path::Path;

pub fn record(body: &[u8]) -> Vec<u8> {
    let mut b = (body.len() as u32).to_le_bytes().to_vec();
    b.extend(body);
    b
}

pub fn compound(path: &Path, streams: &[(&str, Vec<u8>)]) {
    let mut cf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for (name, bytes) in streams {
        if let Some((dir, _)) = name.rsplit_once('/') {
            cf.create_storage_all(dir).unwrap();
        }
        cf.create_stream(name).unwrap().write_all(bytes).unwrap();
    }
    cf.flush().unwrap();
    std::fs::write(path, cf.into_inner().into_inner()).unwrap();
}

pub fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(bytes).unwrap();
    z.finish().unwrap()
}

pub fn pcblib(path: &Path) {
    let mut list = record(b"|HEADER=PCB Library\0");
    list.extend(1u32.to_le_bytes());
    list.extend(record(b"\x01A"));
    compound(path, &[
        ("Library/Data", list),
        ("A/Data", record(b"\x01A")),
        ("A/Parameters", record(b"|PATTERN=A|DESCRIPTION=Original|HEIGHT=1mil\0")),
        ("Library/Models/Data", record(b"|ID=one|NAME=victim.PcbDoc|EMBED=TRUE\0")),
        ("Library/Models/0", compressed(b"model")),
    ]);
}

pub fn pcb(path: &Path) {
    compound(path, &[
        ("Board6/Data", record(b"|KIND=Protel_Advanced_PCB\0")),
        ("Models/Data", record(b"|ID=one|NAME=victim.PcbDoc|EMBED=TRUE\0")),
        ("Models/0", compressed(b"model")),
    ]);
}

pub fn sch(path: &Path, names: &[&str]) {
    let mut storage = record(b"|HEADER=Icon storage\0");
    for name in names {
        let data = compressed(b"image");
        let mut body = vec![0xd0, name.len() as u8];
        body.extend(name.as_bytes());
        body.extend((data.len() as u32).to_le_bytes());
        body.extend(data);
        storage.extend(record(&body));
    }
    compound(path, &[
        ("FileHeader", record(b"|HEADER=Protel for Windows - Schematic Capture Binary File Version 5.0|WEIGHT=0\0")),
        ("Storage", storage),
    ]);
}
