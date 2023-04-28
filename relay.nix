{ lib
, makeWrapper
, nixosTests
, protobuf
, rustPlatform
, stdenv
}:

rustPlatform.buildRustPackage {
  pname = "relay";
  version = "0.4.0-beta.1";
  src = ./.;
  cargoSha256 = "hzNPI6ODQ208eGQ4xY3dZ+c5O1A3ozUF1emglv3ni/o=";

  PROTOC = "${protobuf}/bin/protoc";
  PROTOC_INCLUDE = "${protobuf}/include";

  nativeBuildInputs = [ ];

  passthru.tests = { inherit (nixosTests) relay; };

  meta = with lib; {
    description = "A simple image hosting service";
    homepage = "https://git.asonix.dog/asonix/relay";
    license = with licenses; [ agpl3Plus ];
  };
}
