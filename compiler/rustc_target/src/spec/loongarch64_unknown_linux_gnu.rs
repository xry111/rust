use crate::spec::{Target, TargetOptions};

pub fn target() -> Target {
    Target {
        llvm_target: "loongarch64-unknown-linux-gnu".to_string(),
        pointer_width: 64,
        data_layout: "e-m:e-p:64:64-i8:8:32-i16:16:32-i64:64-n32:64-S128"
            .to_string(),
        arch: "loongarch64".to_string(),
        options: TargetOptions {
            cpu: "la464".to_string(),
            features: "+f,+d".to_string(),
            llvm_abiname: "lp64d".to_string(),
            max_atomic_width: Some(64),

            ..super::linux_gnu_base::opts()
        }
    }
}
