// Reference: muda/src/platform_impl/macos/util.rs:10.
pub fn strip_mnemonic(text: &str) -> String {
    text.replace("&&", "[~~]")
        .replace('&', "")
        .replace("[~~]", "&")
}
