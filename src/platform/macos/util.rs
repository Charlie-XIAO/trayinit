pub fn strip_mnemonic(text: &str) -> String {
    text.replace("&&", "[~~]")
        .replace('&', "")
        .replace("[~~]", "&")
}
