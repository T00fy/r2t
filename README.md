# r2t (repo-to-text)

`r2t` is a blazing-fast command-line tool, written in Rust, that converts a directory's structure and contents into a single, well-structured text file. This is particularly useful for providing the full context of a codebase to a Large Language Model (LLM).

It is a Rust rewrite inspired by the original Python [repo-to-text](https://github.com/kirill-markin/repo-to-text) by Kirill Markin, with added features like out-of-the-box binary filtering and flexible configuration.

## Key Features

-   **Flexible Output Formats:** Generate output in multiple formats. These formats are not strict, and are more akin to a pseudo format for better output to an LLM:
    -   `xml`: (default) The original repo-to-text output, XML-like.
    -   `yaml`: clean, YAML-like output, slightly more verbose.
    -   `json`: JSON-like output.
-   **Smart Filtering:**
    -   Respects `.gitignore` rules by default.
    -   Automatically detects and excludes binary files like images, archives, and executables (while intelligently including text-based vector graphics like SVG).
    -   Optionally skips the content of test files (e.g., `*_test.go`, `src/test/**`) and inline Rust test modules (`#[cfg(test)]`) to create a more concise context.
-   **Customizable:** Use a `.r2t.yaml` file to define custom ignore patterns and a default output format.
-   **Easy to Use:** Simple and intuitive command-line interface.
-   **Cross-Platform:** Works on Windows, macOS, and Linux.

## Installation

You can really quickly try this out by just simply running
```bash
    cargo install r2t
```

Or if you wanted to install from source:

1.  Clone the repository:
    ```bash
    git clone https://github.com/T00fy/r2t.git
    cd r2t
    ```
2.  Build and install the binary:
    ```bash
    cargo install --path .
    ```
    The executable `r2t` will now be available in your Cargo bin path.

## Usage

### Basic Usage

Running `r2t` in a project directory will process it and create a timestamped output file (e.g., `repo-to-text_1700000000.yaml`) in the same directory. The default output format is XML.

```bash
# Process the current directory (creates a .yaml file)
r2t

# Process a specific directory
r2t /path/to/your-project
```

### Command-Line Options

```
Usage: r2t [OPTIONS] [PATH]

Arguments:
  [PATH]  The root directory of the repository to process [default: .]

Options:
  -o, --output-dir <OUTPUT_DIR>  Directory to save the output file. Defaults to the input directory
      --format <FORMAT>          The output format for the final text file [possible values: yaml, json, xml]
      --stdout                   Output the result to stdout instead of a file
      --no-gitignore             Do not respect .gitignore files for filtering
      --skip-tests               Skip including the content of test files and inline test modules
      --create-settings          Create a default .r2t.yaml settings file in the current directory
      --global                   Use with --create-settings to create a global configuration file
  -h, --help                     Print help
  -V, --version                  Print version
```

**Examples:**

-   **Create a JSON-like format file:**
    ```bash
    r2t --format json
    ```

-   **Save to a different directory:**
    ```bash
    r2t /path/to/project --output-dir /path/to/output
    ```

-   **Include files that are normally ignored by `.gitignore`:**
    ```bash
    r2t --no-gitignore
    ```

-   **Exclude test file content: (Currently only java/go/rust/python is supported)**
    ```bash
    r2t --skip-tests
    ```

## Configuration

You can customize which files are included and the default format by creating a `.r2t.yaml` file in your project's root directory. `r2t` will automatically use it. You can also create a global config file.

To generate a default config file, run:

```bash
# Create .r2t.yaml in the current directory
r2t --create-settings

# Create a global config file in the system's config directory
r2t --create-settings --global
```

The default configuration looks like this:

```yaml
# r2t settings file - https://github.com/T00fy/r2t
# Syntax: gitignore-style glob patterns

# The output format. Can be: yaml, json, xml
# Defaults to xml if not specified.
# format: xml

# Ignore files and directories for both the tree view and content sections.
ignore-tree-and-content:
  - ".git/"
  - "target/"
  - "node_modules/"
  - ".idea/"
  - "*.log"
  - ".terraform/"

# Ignore files only for the content section (they will still appear in the tree).
ignore-content:
  - "LICENSE"
  - "*.lock"
  - ".r2t.yaml"
```

-   `format`: Sets the default output format when the `--format` flag is not used.
-   `ignore-tree-and-content`: Patterns for files/directories to be **completely excluded** from both the directory tree and the content output.
-   `ignore-content`: Patterns for files to be **excluded from the content section**, but still be visible in the directory tree. This is useful for things like lock files or licenses that you want the LLM to know exist but don't need the contents of.

## Development

### Building

```bash
cargo build --release
```
The binary will be located at `target/release/r2t`.

### Testing

Run the full test suite:
```bash
cargo test
```

## Acknowledgements
-   This project is a Rust rewrite inspired by the excellent Python tool [repo-to-text](https://github.com/kirill-markin/repo-to-text) by Kirill Markin.