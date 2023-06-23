{ lib
, nixosTests
, protobuf
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "relay";
  version = "0.3.85";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

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
