{
  description = "jscan development and benchmark shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          python = pkgs.python3.withPackages (ps: [
            ps.genson
          ]);
          jsont = pkgs.buildGoModule {
            pname = "jsont";
            version = "0-unstable-2026-05-16";
            src = pkgs.fetchFromGitHub {
              owner = "okaris";
              repo = "jsont";
              rev = "22b849be807f1054b9c0db5d2f231326654ad49a";
              hash = "sha256-KD64HWh3AIFhQC11+RAmi5zhuFfS0A+biHQRVmHx9tw=";
            };
            vendorHash = null;
            subPackages = [ "cmd/jt" ];
            postInstall = ''
              ln -s "$out/bin/jt" "$out/bin/jsont"
            '';
          };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy

              nodejs
              python
              uv
              go
              cargo-binstall

              jq
              jaq
              ripgrep
              jsongrep
              jsont

              quicktype
              gron
              fastgron
              duckdb

              hyperfine
              coreutils
            ];

            shellHook = ''
              echo "jscan dev shell: Rust, Node, jq/jaq/rg/jg, jt/jsont, quicktype, gron/fastgron, duckdb"
              echo "extra competitors still need bootstrap: genson-cli, json-to-schema, schemax-cli, drivel"
            '';
          };
        }
      );
    };
}
