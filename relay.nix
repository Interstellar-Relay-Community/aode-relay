{ lib
, nixosTests
, protobuf
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "relay";
  version = "0.3.104";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  PROTOC = "${protobuf}/bin/protoc";
  PROTOC_INCLUDE = "${protobuf}/include";
  RUSTFLAGS = "--cfg tokio_unstable";

  nativeBuildInputs = [ ];

  passthru.tests = { inherit (nixosTests) relay; };

  meta = with lib; {
    description = "An ActivityPub relay";
    homepage = "https://git.asonix.dog/asonix/relay";
    license = with licenses; [ agpl3Plus ];
  };
}
