extern crate embed_resource;
fn main() {
    embed_resource::compile("open-share-manifest.rc", embed_resource::NONE)
        .manifest_optional().expect("manifest compilation failed");
}
