wit_bindgen::generate!({
    path: "../../crates/protocol-wasm/wit",
    world: "identity-protocol-package"
});
use super::Package;
export!(Package);
