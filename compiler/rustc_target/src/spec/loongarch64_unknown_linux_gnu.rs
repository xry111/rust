use crate::spec::{Target, TargetOptions};

pub fn target() -> Target {
    Target {
        llvm_target: "loongarch64-unknown-linux-gnu".into(),
        pointer_width: 64,
        data_layout: "e-m:e-p:64:64-i8:8:32-i16:16:32-i64:64-n32:64-S128"
            .into(),
        arch: "loongarch64".into(),
        options: TargetOptions {
            cpu: "la464".into(),
            features: "+f,+d".into(),
            llvm_abiname: "lp64d".into(),
            max_atomic_width: Some(64),

            ..super::linux_gnu_base::opts()
        }
    }
}
