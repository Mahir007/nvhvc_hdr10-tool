//! Whether the Windows App Runtime the app needs is installed, for the installer.

use windows::Win32::Security::PSID;
use windows::Win32::Storage::Packaging::Appx::{
    CreatePackageDependencyOptions_None, DeletePackageDependency, PACKAGE_VERSION,
    PACKAGE_VERSION_0, PackageDependencyLifetimeKind_Process,
    PackageDependencyProcessorArchitectures_Neutral, PackageDependencyProcessorArchitectures_X64,
    TryCreatePackageDependency,
};
use windows::core::{PCWSTR, w};

/// The oldest runtime windows-reactor 0.100 runs on, 2.4.0.0, as major, minor, build and revision
/// in 16 bits each. Keep it in step with the runtime windows-reactor-setup stages.
const MIN_VERSION: u64 = (2 << 48) | (4 << 32);

/// Whether the current user has the Windows App Runtime the app needs, found the way the app
/// finds it at startup. The package dependency this creates is deleted again.
pub fn runtime_installed() -> bool {
    let min = PACKAGE_VERSION {
        Anonymous: PACKAGE_VERSION_0 {
            Version: MIN_VERSION,
        },
    };
    let created = unsafe {
        TryCreatePackageDependency(
            PSID::default(),
            w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe"),
            min,
            PackageDependencyProcessorArchitectures_X64
                | PackageDependencyProcessorArchitectures_Neutral,
            PackageDependencyLifetimeKind_Process,
            PCWSTR::null(),
            CreatePackageDependencyOptions_None,
        )
    };
    match created {
        Ok(id) => {
            let _ = unsafe { DeletePackageDependency(PCWSTR(id.0)) };
            true
        }
        Err(_) => false,
    }
}
