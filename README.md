# halld

A linker for HAL's "relocatable data" in Super Smash Bros. 64.

## Usage
```bash
Usage:
    halld [options] [-L dir]... <script> [-o output.o]
    halld -h | --help
    halld -V | --version

Args:
    <script>    path to a JSON linker script

Options:
    -L --search-dir        Zero or more directories in which to search for 
                           files named in <script>
    -o --output            Path to output object; if passed, this overides the
                           settings.output field in <script>
    -c --header            Path to output a C header file with file id defines
    -d --dependency-file   Path to output a Makefile dep (.d) file
    -h --help              Print this help message
    -V --version           Print version information
```

### Link Script JSON
The script is a simple format with two main keys: `"settings"` and `"script"`. The `"settings"` key is an Object for entering the same info as the CLI options. The `"script"` key is an array of files to link

#### `"settings"`
| Key          | Necessary | Value | Description |
|--------------|-----------|-------|-------------|
| `searchDirs` | false     | str[] | A list of directory paths to check. Added to list pased with CLI option `-L`|
| `output`     | false     | str   | Path to output linked objected |

#### `"script"`
This is an array of files to link into one object. It supports both directly linking in binary data, as well as relocatable ELF objects. 

