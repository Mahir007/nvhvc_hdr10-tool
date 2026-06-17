//! Stages a self-contained Windows App SDK Runtime next to the exe (the runtime DLLs + bootstrap),
//! so the app runs without the runtime being installed system-wide.
fn main() {
    windows_reactor_setup::as_self_contained();
}
