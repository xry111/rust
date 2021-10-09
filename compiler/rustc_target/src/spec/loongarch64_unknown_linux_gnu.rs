use crate::spec::{LinkerFlavor, Target};

pub fn target() -> Target {
    let mut base = super::linux_gnu_base::opts();
    base.cpu = "loongarch64".to_string();
    base.max_atomic_width = Some(64);
    base.pre_link_args.entry(LinkerFlavor::Gcc).or_default().push("-mabi=lp64".to_string());

    Target {
        llvm_target: "loongarch64-unknown-linux-gnu".to_string(),
        pointer_width: 64,
        data_layout: "e-m:e-p:64:64-i64:64-i128:128-n64-S128"
            .to_string(),
        arch: "loongarch64".to_string(),
        options: base,
    }
}
