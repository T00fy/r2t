{
  description = "r2t - A fast CLI tool to convert a repository's structure and contents into a single text file for LLM context";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "r2t";
            version = "0.4.0";

            src = ./.;

            cargoHash = "sha256-Y3trFaKVLLv1l82I8ZjaVtEF0aET4mySBFtOCIgSL3Q=";

            meta = with pkgs.lib; {
              description = "A fast CLI tool to convert a repository's structure and contents into a single text file for LLM context";
              longDescription = ''
                r2t is a blazing-fast command-line tool that converts a directory's 
                structure and contents into a single, well-structured text file. 
                Supports XML, YAML, and JSON output formats. Features smart filtering 
                with .gitignore support and automatic binary file exclusion.
              '';
              homepage = "https://github.com/T00fy/r2t";
              license = licenses.mit;
              maintainers = [ ];
              mainProgram = "r2t";
              platforms = platforms.unix;
            };
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt
            ];

            RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          };
        });
    };
}
