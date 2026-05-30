{
  description = "harnessd saved-file inline completion daemon";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      harnessd = pkgs.rustPlatform.buildRustPackage {
        pname = "harnessd";
        version = "0.1.0";
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            let name = builtins.baseNameOf (toString path);
            in !(builtins.elem name [ ".git" "target" "result" "nvim.log" ]);
        };
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.makeWrapper ];
        postInstall = ''
          wrapProgram $out/bin/harnessd \
            --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.codex-acp ]}
        '';
      };
    in
    {
      packages.${system} = {
        inherit harnessd;
        default = harnessd;
      };

      apps.${system}.default = {
        type = "app";
        program = "${harnessd}/bin/harnessd";
        meta.description = "Run the harnessd inline completion daemon CLI";
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          rustc
          rustfmt
          clippy
          just
          codex-acp
          codex
        ];
      };

      checks.${system}.default = harnessd;
    };
}
