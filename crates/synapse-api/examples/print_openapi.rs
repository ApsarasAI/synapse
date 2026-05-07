fn main() {
    let spec = synapse_api::server::openapi_document();
    println!("{}", spec.to_pretty_json().expect("serialize OpenAPI"));
}
