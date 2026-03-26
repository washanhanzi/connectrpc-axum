#[test]
fn generated_builder_enforces_semantic_handler_signatures() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/semantic_handler_valid.rs");
    tests.compile_fail("tests/ui/semantic_handler_invalid_server_stream.rs");
}
