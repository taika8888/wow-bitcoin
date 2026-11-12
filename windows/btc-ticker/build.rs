//! The linker embeds the manifest and the icon itself, so the build needs neither rc.exe nor a C compiler.

use std::path::Path;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crate_dir = Path::new(&crate_dir);
    let resources = Path::new(&std::env::var("OUT_DIR").unwrap()).join("app.res");
    std::fs::write(&resources, icon_resources(&std::fs::read(crate_dir.join("app.ico")).unwrap())).unwrap();
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", crate_dir.join("app.manifest").display());
    println!("cargo:rustc-link-arg-bins={}", resources.display());
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.ico");
}

/// A .res file holding each image of the .ico as an RT_ICON and their directory as RT_GROUP_ICON 1. Explorer shows
/// the group with the lowest id as the exe's icon.
fn icon_resources(ico: &[u8]) -> Vec<u8> {
    let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes([ico[4], ico[5]]);
    let mut resources = resource(0, 0, &[]);
    let mut group = [0, 1, count].map(u16::to_le_bytes).concat();
    for id in 1..=count {
        let entry = 6 + 16 * usize::from(id - 1);
        let (length, offset) = (u32_at(entry + 8), u32_at(entry + 12));
        resources.extend(resource(RT_ICON, id, &ico[offset..offset + length]));
        group.extend(&ico[entry..entry + 12]);
        group.extend(id.to_le_bytes());
    }
    resources.extend(resource(RT_GROUP_ICON, 1, &group));
    resources
}

/// One entry with a numeric type and name, language neutral, padded to four bytes.
fn resource(kind: u16, name: u16, data: &[u8]) -> Vec<u8> {
    let mut entry = [data.len() as u32, 32].map(u32::to_le_bytes).concat();
    entry.extend([0xFFFF, kind, 0xFFFF, name].map(u16::to_le_bytes).concat());
    entry.extend([0; 16]);
    entry.extend(data);
    entry.resize(entry.len().next_multiple_of(4), 0);
    entry
}
