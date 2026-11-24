use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    let ico_path = PathBuf::from("assets/app.ico");
    let Ok(ico) = fs::read(&ico_path) else {
        return;
    };
    if ico.len() < 6 {
        return;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    if ico.len() < 6 + count * 16 {
        return;
    }

    let mut res = Vec::new();
    // Standard 32-byte empty resource header
    push_res_record(&mut res, &[], &[], 0, 0, &[], 0);

    // Build RT_GROUP_ICON data (NEWHEADER + GRPICONDIRENTRY[])
    let mut group_data = Vec::with_capacity(6 + count * 14);
    group_data.extend_from_slice(&ico[0..6]);

    for i in 0..count {
        let base = 6 + i * 16;
        let entry = &ico[base..base + 16];
        let bytes_in_res = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let image_offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        if image_offset + bytes_in_res > ico.len() {
            panic!("Invalid ICO entry {i}");
        }
        let icon_id = (i + 1) as u16;
        let frame = &ico[image_offset..image_offset + bytes_in_res];

        // GRPICONDIRENTRY is first 12 bytes of ICONDIRENTRY + u16 id
        group_data.extend_from_slice(&entry[0..12]);
        group_data.extend_from_slice(&icon_id.to_le_bytes());

        // RT_ICON = 3
        push_int_res(&mut res, 3, icon_id, 0x1010, frame);
    }

    // RT_GROUP_ICON = 14, ID = 1
    push_int_res(&mut res, 14, 1, 0x1030, &group_data);

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("app.res");
    fs::write(&res_path, res).expect("write app.res");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let obj_path = out_dir.join("app_res.o");
        let windres = ["windres", "x86_64-w64-mingw32-windres", "llvm-windres"]
            .into_iter()
            .find(|tool| {
                std::process::Command::new(tool)
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
            .expect("windres not found for gnu target");
        let status = std::process::Command::new(windres)
            .args([
                "-i",
                res_path.to_str().expect("utf-8 res path"),
                "-o",
                obj_path.to_str().expect("utf-8 obj path"),
                "-O",
                "coff",
            ])
            .status()
            .expect("run windres");
        assert!(status.success(), "windres failed");
        println!("cargo:rustc-link-arg={}", obj_path.display());
    } else {
        println!("cargo:rustc-link-arg={}", res_path.display());
    }
}

fn push_int_res(out: &mut Vec<u8>, res_type: u16, res_id: u16, memory_flags: u16, data: &[u8]) {
    let type_bytes = [0xFF, 0xFF, res_type as u8, (res_type >> 8) as u8];
    let name_bytes = [0xFF, 0xFF, res_id as u8, (res_id >> 8) as u8];
    push_res_record(out, &type_bytes, &name_bytes, memory_flags, 0x0409, data, 0);
}

fn push_res_record(
    out: &mut Vec<u8>,
    type_bytes: &[u8],
    name_bytes: &[u8],
    memory_flags: u16,
    language_id: u16,
    data: &[u8],
    data_version: u32,
) {
    let type_field: &[u8] = if type_bytes.is_empty() {
        &[0xFF, 0xFF, 0, 0]
    } else {
        type_bytes
    };
    let name_field: &[u8] = if name_bytes.is_empty() {
        &[0xFF, 0xFF, 0, 0]
    } else {
        name_bytes
    };

    let unaligned_header_size = 8 + type_field.len() + name_field.len() + 16;
    let header_pad = (4 - (unaligned_header_size % 4)) % 4;
    let header_size = (unaligned_header_size + header_pad) as u32;
    let data_size = data.len() as u32;

    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(type_field);
    out.extend_from_slice(name_field);
    out.extend(std::iter::repeat_n(0u8, header_pad));
    out.extend_from_slice(&data_version.to_le_bytes());
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&language_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);

    let tail_pad = (4 - (data.len() % 4)) % 4;
    out.extend(std::iter::repeat_n(0u8, tail_pad));
}
