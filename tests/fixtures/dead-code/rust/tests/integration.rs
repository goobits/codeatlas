mod support;

use self::support::fixture_value;

#[test]
fn public_api_works() {
    assert_eq!(codeatlas_rust_fixture::public_api(), "used");
    assert_eq!(fixture_value(), "shared");
}
