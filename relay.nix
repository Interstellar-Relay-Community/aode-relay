{ lib
, nixosTests
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "relay";
  version = "0.3.106";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  RUSTFLAGS = "--cfg tokio_unstable";

  nativeBuildInputs = [ ];

  passthru.tests = { inherit (nixosTests) relay; };

  meta = with lib; {
    description = "An ActivityPub relay";
    homepage = "https://git.asonix.dog/asonix/relay";
    license = with licenses; [ agpl3Plus ];
  };
}
