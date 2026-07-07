[![Rust](https://github.com/steveking-gh/firmion/actions/workflows/rust.yml/badge.svg)](https://github.com/steveking-gh/firmion/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/steveking-gh/firmion/graph/badge.svg?v=1)](https://codecov.io/gh/steveking-gh/firmion)
[![rust report card](https://rust-reportcard.xuri.me/badge/github.com/steveking-gh/firmion?v=1)](https://rust-reportcard.xuri.me/report/github.com/steveking-gh/firmion)
[![Audit Check](https://github.com/steveking-gh/firmion/actions/workflows/audit-check.yml/badge.svg?branch=master)](https://github.com/steveking-gh/firmion/actions/workflows/audit-check.yml)
![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)

# Firmion

Firmion is a domain specific language for linking and composing of an output
file. Firmion simplifies construction of complex files by managing sizes,
offsets and ordering in a readable declarative style.  Firmion tries to be
especially useful when creating FLASH, ROM or other non-volatile memory images.

# Features

## Support For All Common Firmware Image Structures

- Define, compose and nest content sections to create your output
- Define platform specific memory regions to set address and size boundaries
- Write raw data, strings, and repeated values
- Write hashes and checksums
- Write offsets, addresses and sizes
- Full globbing support for reading ELF object files and object file sections
- Copy external files into your output
- Align and pad content
- Use little or big-endian byte order
- Output detailed map files in various formats: Rust, C header, JSON and CSV

## Firmion Language Features

- Thorough reference documentation
- Comfortable curly-brace and semicolon syntax
- Declarative style so source code resembles the output file
- Include other `.firm` files for modularity
- Robust address and offset management with full [section](#section) scope
  support
- Full support for arithmetic expressions
- Conditional expressions with if/else
- Compile-time interface for user-defined extensions
- Handy shorthand for numeric values, e.g. 4M is 4 x 1024 x 1024.
- Label any output location for easy reference
- Support for "-D" command-line definitions visible to your program, e.g.
  "-DMEM_SIZE=1M"

## Debug And Diagnostic Features

- Use [`assert`](#assert) statements to provide inline validation of your program
- Use [`print`](#print) statements for debug and or any other console messages
- Use [`trace`](#trace) statements to peek into Firmion's iterative image
  generation process
- Firmion provides clear error messages with full source code context
- Optional verbose debug output levels
- Firmion has hundreds of integration tests exercising features and edge cases
- Firmion is actively fuzz tested against panics

## Cross-Platform Support

- Implemented in Rust with support for any Rust language host platform
- Fully open source with MIT license

# Quick Start

## Install Firmion with Cargo

If you already have the Rust development tools installed, just install using
[cargo](https://doc.rust-lang.org/cargo/).

    cargo install firmion

## Install Prebuilt Binaries for Linux

    curl --proto '=https' --tlsv1.2 -LsSf https://github.com/steveking-gh/firmion/releases/download/0.8.0/firmion-installer.sh | sh

## Install Prebuilt Binaries for Windows

Start a command prompt and execute the following:

    powershell -ExecutionPolicy Bypass -c "irm https://github.com/steveking-gh/firmion/releases/download/0.8.0/firmion-installer.ps1 | iex"

## Build From Source

### Step 1: Install Rust

Firmion is written in rust, which works on all major operating systems.
Installing rust is simple and documented in the [Rust Getting
Started](https://www.rust-lang.org/learn/get-started) guide.

### Step 2: Clone Firmion

From a command prompt, clone Firmion and change directory to your clone.  For
example:

    git clone https://github.com/steveking-gh/firmion.git
    cd firmion

### Step 3: Build and Run Self-Tests

    cargo test --release --all

All tests should pass, 0 tests should fail.

### Step 4: Install Firmion

The previous build step created the Firmion binary as
`./target/release/firmion`. You can install the Firmion binary anywhere on your
system.  As a convenience, cargo provides a per-user installation as
`$HOME/.cargo/bin/firmion`.

    cargo install --path ./

---

# What Can Firmion Do?

Firmion can assemble any number of input files into a unified output.

<img src="./images/unified_binary.svg" width="400">

---

Firmion can calculate relative or absolute offsets, allowing your output to
contain pointer tables, cross-references and so on.

<img src="./images/offsets.svg" width="400">

---

Firmion can add pad bytes to force parts of the file to be a certain size.

<img src="./images/pad.svg" width="400">

---

Firmion can add pad bytes to force parts of the file to start at an aligned
boundary or at an absolute location.

<img src="./images/align.svg" width="400">

---

Firmion can write your own strings and data defined within your Firmion source
file.

<img src="./images/adhoc.svg" width="400">

---

Firmion provides full featured assert and print statement support to help with
debugging complex output files.

<img src="./images/debug.svg" width="400">

---

## Hello World

For a source file called hello.firm:

    /*
     * A section defines part of an output.
     */
    section foo {
        // Print a quoted string to the console
        print "Hello World!\n";
    }

    // An output statement outputs the section to a file
    output foo;

Running Firmion on the file produces the expected message:

    $ firmion hello.firm
    Hello World!
    $

Firmion also produced an empty file called `output.bin`.  This file is the
default output when you don't specify some other name on the command line with
the `-o` option.  Why is the file empty?  Because nothing in our program
produced output file content -- we just printed the console message.

Let's fix that.  We can replace the `print` command with the `wrs` command,
which is shorthand for 'write string':

    /*
     * A section defines part of an output.
     */
    section foo {
        // Write a quoted string to the output
        wrs "Hello World!\n";
    }

    // An output statement outputs the section to a file
    output foo;

Now, running the command again:

    $ firmion hello.firm
    $

Produces output.bin containing the string `Hello World!\n`.

# Basic Structure of a Firmion Program

A Firmion source file consists of one or more [section](#section) definitions
and exactly one [output](#output-statement) statement. The output statement
specifies the top-level section that defines the output file. Starting from this
top section, Firmion recursively evaluates each nested section and its commands
to produce the output file.  For example, we can define a section with a
write-string ([wrs](#write-string)) command:

    section foo {        // Start a new section named 'foo'
        wrs "I'm foo";   // wrs writes a string into the section.
    }

    output foo;          // Final output

Produces a default output named `output.bin`.

    $ cat output.bin
    I'm foo

Using the [wr](#write) command, sections can embed other sections:

    section foo {
        wrs "I'm foo\n";
    }

    section bar {
        wrs "I'm bar\n";
        wr foo;           // nested section
    }

    output bar;

Produces `output.bin`:

    $ cat output.bin
    I'm bar
    I'm foo

Users can also define [memory regions](#region) that describe hardware memory layout
on the target platform.  Regions provide convenient overflow checking without
the need for explicit [assert](#assert) statements.

    region FLASH {
        addr = 0xF000_000;  // Starting address of the region
        size = 1M;          // Size of the region = 1MB
        default_pad_byte = 0xFF;  // 0xFF is the region default, but shown for completeness.
    }

Exactly one [section](#section) can declare itself `in` a region.  We call this
section the *bound section* of a region.  Of course, any nested section in the
bound section is also in the region.

    section boot_section { ... }
    section runtime_section { ... }

    section firmware_top in FLASH {
        wr boot_section;
        wr runtime_section;
    }

To provide fine grain bounds control, you can even nest sections that are *also in
their own region*.  Firmion internally understands this creates a region
intersection and applies the constraints of *all* parent regions.

    // Total FLASH space
    region FLASH {
        addr = 0xF000_000;  // Starting address of the region
        size = 1M;          // Size of the region = 1MB
        default_pad_byte = 0xFF;  // 0xFF is the region default, but shown for completeness.
    }

    // FLASH space reserved for boot loader
    region BOOT_FLASH {
        addr = 0xF000_000;  // Starting address of the region
        size = 64K;         // Size of boot reserved area is 64KB
    }

    section boot_section in BOOT_FLASH { ... }
    section runtime_section { ... }

    section firmware_top in FLASH {
        wr boot_section;  // Nested section with its own region carve-out
        pad_sec_offset(sizeof(BOOT_FLASH));  // pad to keep runtime out of reserved boot area
        wr runtime_section;
    }

Users can extend Firmion with custom data processing using [Firmion
extensions](#firmion-extensions).  Users write the output of their extension
call into a [section](#section) with a `wr`.

    section foo {
        wrs "I'm foo\n";
    }

    section bar {
        wrs "I'm bar\n";
        wr foo;           // nested section
    }

    section final {
        wr bar;
        wr my_stuff::crc(bar);  // Write a 4 byte CRC hash for section 'bar'.
    }

    assert(sizeof(final) == 20);
    output final;

---

# Assert and Print

To aid in debug, Firmion supports `assert` and `print` statements in your
programs.

Assert expressions automate error checking.  This example verifies our
expectation that [section](#section) 'bar' is 13 bytes long.

    section bar {
        wrs "Hello World!\n";
        assert sizeof(bar) == 13;
    }
    output bar;

You can print this length information to the console during generation of your
output:

    section bar {
        print "Output size is ", sizeof(bar), " bytes\n";
        wrs "Hello World!\n";
        assert sizeof(bar) == 13;
    }
    output bar;

Prints the console message:

    Output size is 13 bytes

---

# Addresses and Offsets

Unlike the [GNU linker
'ld'](https://ftp.gnu.org/old-gnu/Manuals/ld-2.9.1/html_mono/ld.html) concept of
a *location counter*, Firmion uses *scoped addresses* and *scoped offsets* to
track locations.  **Addresses and offsets are 64-bit unsigned values that mark
the position of the *next* byte of output**.  Firmion allows users to reference
and manipulate these values, adding pad bytes as necessary.

Importantly, addresses and offsets are *scoped* to their enclosing section. When
entering a nested (child) section, Firmion saves the outer (parent) section's
inflight address and offset values.  When exiting a child section, Firmion
restores and updates the parent's address and offset values.  From the
perspective of the parent section, a child [section](#section) is a `wr` with
the parent's addresses and offsets updated per the size of the child.

For the specific case of the address and address offset, a child section
inherits these values by default from the parent section.  If the child section
does not use `set_addr`, then the address and address offset simply continue
growing in step with the parent.

The only global (non-scoped) offset is the `file_offset`.  Starting from 0, this
value monotonically increases to the end of the output file.

The following table provides a summary of the addresses and offsets used in
Firmion.

| Variable       | Section Entry | Section Exit     | [`set_addr`](#set_addr) | [`pad_sec_offset`](#pad_sec_offset) | [`pad_addr_offset`](#pad_addr_offset) | [`pad_file_offset`](#pad_file_offset) |
| -------------- | ------------- | ---------------- | ----------------------- | ----------------------------------- | ------------------------------------- | ------------------------------------- |
| Address        | No Change     | Restore & Update | Set                     | Pad Forward                         | Pad Forward                           | Pad Forward                           |
| Address Offset | No Change     | Restore & Update | Set to 0                | Pad Forward                         | Pad Forward                           | Pad Forward                           |
| Section Offset | Set to 0      | Restore & Update | No change               | Pad Forward                         | Pad Forward                           | Pad Forward                           |
| File Offset    | No Change     | No Change        | No Change               | Pad Forward                         | Pad Forward                           | Pad Forward                           |

The following diagram shows several address and offset concepts.  Users specify
the starting logical address of the output [section](#section) `D` using a
region. Alternatively, users can change the address within `D` using
[set_addr](#set_addr) at the top of the output section.

<figure>
  <img src="./images/scoped_address_plain_no_text.svg" width="100%"alt="Scoped Address and Offset Example Image">
  <figcaption>In this example, section D is the top level binary output and includes three other sections A, B and C.  The starting address of section D is 0x8000 because the user placed D in FLASH region.  Section C is special and defines its own starting address.  A boot loader can find section C using the pointer at the beginning of the file, then copy section C to its proper starting address of 0xF000.  Notice that <code>addr(D)</code> used in the context of section D returns 0x8C00, not the starting address value 0xF000 nested within section C.
  </figcaption>
</figure>

## Firmion Disallows Address Overwrites

By address, Firmion tracks all bytes written to the output.  Firmion reports an
error if a program's offset or address manipulations cause more than one write
to the same address.

## Firmion Disallows Negative Offset Changes

Firmion enforces that set offset commands must specify an offset change greater
or equal to 0.  Firmion emits pad bytes into the output for any offset change
greater than 0.

## Firmion Disallows Address and Offset Overflows

Firmion emits an error if an address or offset change causes 64-bit unsigned
overflow.  In other words, programs cannot use unsigned overflow wrapping back
to 0.

# Order of Execution

As a mental model, users can think of program execution as occurring in *output
order*.  In other words, an operation producing the first byte of the output
will execute before an operation producing the second byte.

Within a section, output order and source code order are the same. Outside of a
section, output order and source code order may differ.  For example, source
code may declare sections in a different order than instantiated into in the
output.

For top-level statements, i.e. statements outside of a [section](#section)
declaration, Firmion orders execution as follows:

1. Top level statements *before* the [`output`](#output) statement execute
   *before* Firmion begins producing output bytes.
2. The location of the output statement dictates the location of all
   output-ordered statements relative to top level statements like
   [`print`](#print) and [`assert`](#assert).
3. Top level statements *after* the output statement execute *after* Firmion
   produces the output file.

For example:

    print "Start!";
    section B {
        wr A;
        print "B1\n";
        print "B2\n";
    }

    section A {
        print "A1\n";
        print "A2\n";
    }

    print "Top1\n";
    print "Top2\n";
    output B;
    print "Finish!";

Produces the output:

    Start!
    Top1
    Top2
    A1
    A2
    B1
    B2
    Finish!

## Output Creation Phases

This section provides an overview of Firmion's internal output creation phases.

1. `const` **Evaluation Phase**: First, Firmion evaluates all const expressions.
   This phase includes evaluation of all `if/else` statements and the dependent
   const-time operations such as `include` statements in the taken path.
2. **Layout Phase**: Next, Firmion iteratively evaluates all expressions that
   affect output size and layout.  For example, Firmion evaluates `align`
   expressions and extension `size()` calls during this phase.  Firmion skips
   data generation, since knowing the size of operations suffices to determine
   the precise output structure.  This phase completes when successive layout
   iterations produce identical results.
3. **Generate Phase 1**: Next, Firmion begins populating data values into the
   output.  In this first generation phase, Firmion first evaluates `wr`
   statements that do NOT call extensions.  Firmion evaluates wr calls in output
   order.
4. **Generate Phase 2**: Next, Firmion evaluates `wr` statement that call an
   [extension](#firmion-extensions).  Like before, Firmion evaluates extension
   calls in output order.  Firmion executes all extension calls serially on the
   engine thread.
5. **Validation Phase**: Finally, Firmion evaluates `assert` statements, including
   those that call extensions.  Note that Firmion may take an early exit in any
   phase if an `assert` statement will unambiguously fail.

---

# Examples

This section provides realistic Firmion examples tested on the corresponding hardware.

## ESP32-S3

ESP series microcontrollers have complex and unique firmware image requirements.
In the ESP ecosystem, the python based
[esptool](https://github.com/espressif/esptool) hides these formatting headaches
from users.  The build tool [SCons](https://scons.org) drives the overall
source-to-firmware process.  For testing, we hooked the SCons build scripts to
invoke Firmion to create the firmware binary image.  This allowed Firmion to
replace esptool's `elf2image` conversion.

### ESP32 Firmware Image Format

The [ESP32 firmware image
format](https://docs.espressif.com/projects/esptool/en/latest/esp32s3/advanced-topics/firmware-image-format.html)
uses a two-part file header followed by a number of *segments*.  Each segment
has an 8-byte header containing 32b little-endian address and segment size
values.  The following diagram gives an overview.

| Byte offset   | Size | Description          |
| ------------- | ---- | -------------------- |
| 0             | 8    | File Header          |
| 8             | 16   | Extended File Header |
| 24            | 8    | Segment 0 Header     |
| 32            | Len0 | Segment 0 Payload    |
| 32 + Len0     | 8    | Segment 1 Header     |
| 32 + Len0 + 8 | Len1 | Segment 1 Payload    |
| And so on...  | ...  | ...                  |

In the File Header, byte 0 contains the magic byte `0xE9` and byte 1 contains
the number of segments.  During firmware upload, the device's bootloader reads
this format and copies each segment to the specified load address in the
segment's header.

### ESP32 Quirks

ESP32 has a few quirky requirements:

- The ESP32 performs a memory remapping trick for "instruction ROM" (IROM) code
  segments.  The net effect is that the *file offset* of the start of the IROM
  segment payload modulo 64K must equal the *segment's load address* modulo 64K.
- ESP32 does not support a pure padding segments, so the segment *prior* to the
  IROM segment must include padding such that the starting file offset of the
  IROM segment payload meets the requirement above.
- Memory limitations in the bootloader mean that Non-ROM segments cannot
  necessarily take on the padding requirement above.  Creating large (how large?)
  non-ROM segments causes a bootloader overflow of some sort.  The effect is
  boot-looping.  Consequently, the "data ROM" (DROM) segment is the proper
  choice to precede the IROM segment to provide the IROM's required padding.
- The firmware image must contain a legacy single-byte XOR checksum *after* the
  last segment, but exactly one byte *before* the next 16-byte aligned address.
  This checksum must be computed over the *segment payloads* only.

### The `esptool` Reference Result

As driven by SCons, the esptool `elf2image` process created the firmware image
layout shown below.  This table is cut/paste from the `esptool image-info`
command.  Notice the lower 16b of the IROM load address is 0x0020 while the
lower 16b of the file offset is `0x0018`. As described in
[quirks](#esp32-quirks) above, this file offset is no accident.  The lower 16b
of the address and payload file offset must match.  Because the segment header
is 8 bytes, the IROM segment payload starts at `0x10018` + `8` = `0x10020` as
required.

To achieve the required alignment, `elf2image` splits the *data RAM* (DRAM)
content into two segments.  The first DRAM segment serves double duty of loading
*part* of the DRAM content and being the exact length required for required IROM
file offset.  The IROM segment comes next, followed the by another segment with
the remaining DRAM content.  By splitting the DRAM segment, `esptool` avoid
wasting space on pad bytes.  Finally, the IRAM segment comes last and has no
special requirements other than 4 byte alignment.

    Segments Information
    ====================
    Segment   Length   Load addr   File offs  Memory types
    -------  -------  ----------  ----------  ------------
          0  0x0d068  0x3c030020  0x00000018  DROM
          1  0x02f88  0x3fc92a60  0x0000d088  BYTE_ACCESSIBLE, MEM_INTERNAL, DRAM
          2  0x22490  0x42000020  0x00010018  IROM
          3  0x005fc  0x3fc959e8  0x000324b0  BYTE_ACCESSIBLE, MEM_INTERNAL, DRAM
          4  0x0ea60  0x40374000  0x00032ab4  MEM_INTERNAL, IRAM


Not shown in the `esptool image-info` table above are the trailing XOR checksum
and the SHA256 hash over the entire image.

### The Firmion Result

Using the [source code](#esp32-s3-image-source-code) shown below, Firmion can
produce a working ESP32 firmware image with the following layout:

    Segments Information
    ====================
    Segment   Length   Load addr   File offs  Memory types
    -------  -------  ----------  ----------  ------------
          0  0x0fff8  0x3c030020  0x00000018  DROM
          1  0x22490  0x42000020  0x00010018  IROM
          2  0x03584  0x3fc92a60  0x000324b0  BYTE_ACCESSIBLE, MEM_INTERNAL, DRAM
          3  0x0ea60  0x40374000  0x00035a3c  MEM_INTERNAL, IRAM

Creating the complex XOR single-byte checksum required implementing the Firmion
[extension](#firmion-extensions) `esp_checksum` which you can see used in the
[source code](#esp32-s3-image-source-code) below.

### Hooking the SCons Build Scripts

To construct the Firmion version of the firmware, we hook the SCons script near
the end of the process after `esptool` has already created its own binary image.
We copy the `esptool` version to a backup then create the Firmion version.

Code for hooks is in examples/esp32 in the source repo.

### Firmion vs. Esptool Elf2image

First, Firmion currently can extract and write ELF [obj](#obj) content to the
output file as a single contiguous blob.  Instead of splitting the extracted
DRAM [obj](#obj) to achieve the correct IROM offset, the Firmion user can pad the
preceding DROM section. This costs about 12K of otherwise wasted pad bytes.

Secondly, the `elf2image` tool patches the firmware image *inside an extracted
ELF section* with a SHA256 hash of the firmware.elf file.  This hash is separate
from the complete image SHA256 hash at the end of the file.  Firmion can
calculate and write SHA256 hashes such as the trailing SHA256 hash, but cannot
inject them *inside* extracted ELF content, aka an extracted [obj](#obj).  An
[extension](#firmion-extensions) would be required for such a feature.
Consequently, this hash value in the Firmion image is zero. The ESP32 image
upload and execute process does not care about this hash value, so the image
works.

Developers can draw their own readability conclusions, but we contend the
declarative style of the Firmion [source code](#esp32-s3-image-source-code)
makes the structure of firmware images significantly more obvious than reading
the existing `esptool` documentation and source code.  We also note that the
Firmion source file is considerably more compact and maintainable than the
equivalent Python sources in esptool `elf2image` process.

### Conclusion

The `esptool` is the canonical and supported firmware image solution for ESP
based systems.  This experiment is not to replace `esptool`, but to test
Firmion's ability to generate rather notorious set of image requirements.  On
that front, the experiment was a success with addition of `esp_checksum`
[extension](#firmion-extensions) to handle the segment-walking XOR checksum.
Properly replacing the `elf2image` process would at least require a new Firmion
extension to generate the special `firmware.elf` SHA hash and patching the ELF
section.

### ESP32-S3 Image Source Code

```rust
    /*
     * ESP32_S3_test.firm
     * Specify -DFIRMWARE_PATH="path/to/firmware.elf" on the command line.
     */

    // Format reference:
    // https://docs.espressif.com/projects/esptool/en/latest/esp32s3/advanced-topics/firmware-image-format.html

    print "Firmion version ", __FIRMION_VERSION_STRING, "\n";
    print "Firmware ELF is ", FIRMWARE_PATH, "\n";

    // S3 hardware constants based on ESP-IDF and esptool definitions
    const FH_MAGIC_BYTE = 0xE9;
    const FH_SEGMENT_COUNT = 4;
    const FH_FLASH_MODE_DIO = 0x02;
    const FH_FLASH_SIZE_FREQ = 0x4F;    // 16MB, 80MHz
    const FH_ENTRY_POINT = 0x4037708C;
    const FH_CHIP_ID_ESP32S3 = 9;

    // Define the FLASH memory region for our ESP32-S3 target hardware.
    region ext_flash {
        addr = 0x00000000;   // Starting offset of the flash chip
        size = 0x1000000;    // 16MB
    }

    // File Header and Extended File Header.
    section basic_file_header {
        print "Building 8-byte File Header\n";
        wr8 FH_MAGIC_BYTE;
        wr8 FH_SEGMENT_COUNT;
        wr8 FH_FLASH_MODE_DIO;
        wr8 FH_FLASH_SIZE_FREQ;
        wr32 FH_ENTRY_POINT;
        assert sizeof(basic_file_header) == 8;
    }

    const EFH_WP = 0xEE;  // Extended File Header Write Protect byte value
    const EFH_DRIVE_SETTINGS = 0x0;
    const EFH_CHIP_ID_ESP32S3 = FH_CHIP_ID_ESP32S3;
    const EFH_MIN_REV = 0;
    const EFH_MAX_REV = 0xFFFF;
    const EFH_HASH_APPENDED = 1; // Bit 0 indicates if SHA256 hash is appended (it is)

    section extended_file_header {
        print "Building 16-byte Extended File Header\n";
        wr8 EFH_WP;
        wr24 EFH_DRIVE_SETTINGS;
        wr16 EFH_CHIP_ID_ESP32S3;
        wr8 0x00; // Reserved byte
        wr16 EFH_MIN_REV;
        wr16 EFH_MAX_REV;
        wr32 0x00; // Reserved bytes
        wr8 EFH_HASH_APPENDED;
        assert sizeof(extended_file_header) == 16;
    }

    section file_header {
        wr basic_file_header;
        wr extended_file_header;
    }

    const SEG_HEADER_SIZE = 8; // Segment header is 8 bytes: 32b LMA and 32b size

    // Segment 0, FLASH data. This includes the app descriptor and read-only data.
    obj flash_appdesc { file = FIRMWARE_PATH; section = ".flash.appdesc"; }
    assert sizeof(flash_appdesc) > 0;
    obj flash_rodata { file = FIRMWARE_PATH; section = ".flash.rodata"; }
    assert sizeof(flash_rodata) > 0;

    // The IROM segment coming *after* this DROM segment has extreme padding
    // requirements described in the comment below.
    section seg0 {
        print "Building segment 0: DROM, File Offset:  ", file_offset(), "\n";
        print "                          Load Address: ", obj_lma(flash_appdesc), "\n";
        print "                          Size:         ", sizeof(seg0) - SEG_HEADER_SIZE, "\n";
        wr32 obj_lma(flash_appdesc);
        wr32 sizeof(seg0) - SEG_HEADER_SIZE;  // Size of the actual data, excluding the segment header.
        wr flash_appdesc;
        wr flash_rodata;
        wr irom_padding;  // See definition below.
    }

    // There are some strange quirks to IROM. For this segment, the file offset in
    // the image modulo 64K must match the IROM load address modulo 64K! For
    // example, if IROM is at LMA = 0x42000020, then the file offset in the image of
    // the actual ROM code must be 0xnnnn0020, e.g. 0x200020.  Then because the
    // segment has a header of 8 bytes, we have to back up by 8 bytes to 0xnnnn0018.
    // So the final file offset of the segment is 0xnnnn0018. In Firmion, we can
    // achieve this by:
    // 1. Aligning the segment to 64K
    // 2. Additionally pad an amount of bytes equal to: the lower 16 bits of the
    //    LMA, minus the 8 bytes needed for the segment header.
    // 3. Fortunately, the presence of the file header + extended file header and
    //    the first segment header totals 0x20 bytes, which means the IROM segment
    //    will have a LMA 0xnnnn0020 to accommodate.  That lower 0x20 guarantees
    //    enough room for the 8 byte segment header.
    obj flash_text { file = FIRMWARE_PATH; section = ".flash.text"; }
    assert sizeof(flash_text) > 0;
    // Ensure the IROM LMA offset has room for the segment header.  This is
    // essentially guaranteed by the way the ESP32 toolchain linker assigns
    // addresses, but confirm here.
    assert (obj_lma(flash_text) & 0xFFFF) > SEG_HEADER_SIZE;

    section irom_padding {
        align 64K, 0xFF;
        // Now add additional pad bytes equal to the lower 16 bits of the IROM LMA,
        // minus the segment header size.
        wr8 0xFF, (obj_lma(flash_text) & 0xFFFF) - SEG_HEADER_SIZE;
    }

    section seg1 {
        print "Building segment 1: IROM, File Offset:  ", file_offset(), "\n";
        print "                          Load Address: ", obj_lma(flash_text), "\n";
        print "                          Size:         ", sizeof(seg1) - SEG_HEADER_SIZE, "\n";
        // Verify funky offset requirements.  seg1 above handles this headache.
        assert (file_offset() & 0xFFFF) == (obj_lma(flash_text) & 0xFFFF) - SEG_HEADER_SIZE;
        wr32 obj_lma(flash_text);
        wr32 sizeof(seg1) - SEG_HEADER_SIZE;
        wr flash_text;
        align 4;
    }

    obj dram_data { file = FIRMWARE_PATH; section = ".dram0.data"; }
    assert sizeof(dram_data) > 0;

    section seg2 {
        print "Building segment 2: DRAM, File Offset:  ", file_offset(), "\n";
        print "                          Load Address: ", obj_lma(dram_data), "\n";
        print "                          Size:         ", sizeof(seg2) - SEG_HEADER_SIZE, "\n";
        wr32 obj_lma(dram_data);
        wr32 sizeof(seg2) - SEG_HEADER_SIZE;
        wr dram_data;
        align 4;
    }

    obj iram_vectors { file = FIRMWARE_PATH; section = ".iram0.vectors"; }
    assert sizeof(iram_vectors) > 0;
    obj iram_text { file = FIRMWARE_PATH; section = ".iram0.text"; }
    assert sizeof(iram_text) > 0;

    section seg3 {
        print "Building segment 3: IRAM, File Offset:  ", file_offset(), "\n";
        print "                          Load Address: ", obj_lma(iram_vectors), "\n";
        print "                          Size:         ", sizeof(seg3) - SEG_HEADER_SIZE, "\n";
        wr32 obj_lma(iram_vectors);
        wr32 sizeof(seg3) - SEG_HEADER_SIZE;
        wr iram_vectors;
        align 4;  // text sections start on 4 byte aligned.
        wr iram_text;
        align 4;
    }

    section pre_checksum_image {
        print "Building pre_checksum_image...\n";
        wr file_header;
        wr seg0;
        wr seg1;
        wr seg2;
        wr seg3;
        // Starting from the end of the last segment, the ESP bootloader looks
        // for the checksum byte on the next 16 byte boundary, minus 1.
        // So, fill the pre_checksum_image to one byte short of 16 byte alignment.
        // Examples:
        // If sec_offset % 16 is 12, we write (15 - 12) = 3 bytes of padding.
        // If sec_offset % 16 is 0, we write 15 bytes of padding.
        wr8 0x00, 15 - (sec_offset() % 16);
    }

    section pre_sha_image {
        print "Building pre_sha_image...\n";
        wr pre_checksum_image;
        // The checksum extension requires the file image and the offset to the
        // first segment.
        wr std::esp_checksum(pre_checksum_image, sizeof(file_header));
    }

    section firmware in ext_flash {
        print "Building final firmware...\n";
        wr pre_sha_image;
        // SHA256 hash must immediately follow the checksum.
        wr std::sha256(pre_sha_image);
    }

    output firmware;  // Generate the image file.
    print "Firmware image built successfully!\n";
    // Convenient output that resembles esptool's image-info
    print "Segments Information\n";
    print "====================\n";
    print "Segment   Length    File offs\n";
    print "-------   ------    ---------\n";
    print "      0   ", sizeof(seg0) - SEG_HEADER_SIZE, "   ", file_offset(seg0), "\n";
    print "      1   ", sizeof(seg1) - SEG_HEADER_SIZE, "   ", file_offset(seg1), "\n";
    print "      2   ", sizeof(seg2) - SEG_HEADER_SIZE, "   ", file_offset(seg2), "\n";
    print "      3   ", sizeof(seg3) - SEG_HEADER_SIZE, "   ", file_offset(seg3), "\n";
```

---

# Command Line Options Reference

    firmion [OPTIONS] <input>

The required input file contains the Firmion source code to compile and build
the output file.  Firmion source files typically have a .firm file extension.

| Option                     | Description                                                                                                                                                           |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-D<name>[=value]`         | Defines a `const` value from the command line.<br>See [Command-Line Const Defines](#command-line-const-defines) below.                                                |
| `--list-extensions`        | List all available extensions compiled into Firmion as controlled by Cargo feature flags.                                                                               |
| `--max-output-size=<size>` | Reject the output if its size exceeds `<size>` bytes before writing data.<br>Accepts a plain integer or a K/M/G suffix (e.g. `64M`, `512K`, `1G`). Default is `256M`. |
| `--map-csv`                | Writes a CSV format map file `<stem>.map.csv` to the current directory.<br>For example: `firmware.firm` → `firmware.map.csv`.                                        |
| `--map-csv=<file>`         | Writes a CSV map file to the specified file.                                                                                                                          |
| `--map-csv=-`              | Writes a CSV map file to stdout.                                                                                                                                      |
| `--map-c99`                | Writes a C99 header file `<stem>.map.h` to the current directory.<br>For example: `firmware.firm` → `firmware.map.h`.                                                |
| `--map-c99=<file>`         | Writes a C99 header to the specified file.                                                                                                                            |
| `--map-c99=-`              | Writes a C99 header to stdout.                                                                                                                                        |
| `--map-json`               | Writes a JSON format map file `<stem>.map.json` to the current directory.<br>For example: `firmware.firm` → `firmware.map.json`.                                     |
| `--map-json=<file>`        | Writes a JSON map to the specified file.                                                                                                                              |
| `--map-json=-`             | Writes a JSON map to stdout.                                                                                                                                          |
| `--map-rs`                 | Writes a Rust module file `<stem>.map.rs` to the current directory.<br>For example: `firmware.firm` → `firmware.map.rs`.                                             |
| `--map-rs=<file>`          | Writes a Rust module map to the specified file.                                                                                                                       |
| `--map-rs=-`               | Writes a Rust module map to stdout.                                                                                                                                   |
| `--noprint`                | Suppress `print` statement output from the source program.                                                                                                            |
| `-o <file>`                | Output file name. Defaults to `output.bin`.                                                                                                                           |
| `-q`, `--quiet`            | Suppress all console output, including errors. Overrides `-v`. Useful for fuzz testing.                                                                               |
| `-v`                       | Increase verbosity. Repeat up to four times (`-v -v -v -v`).                                                                                                          |

When the user does not specify a path, Firmion writes map file(s) and the output
to the current working directory.

## Command-Line Const Defines

The `-D` option injects a [`const`](#const) definition into the program from the
command line. This option is modelled after the GCC `-D` preprocessor syntax.
You can specify `-D` multiple times, once per each definition.  For example:

    firmion -DBASE=0x8000 -DCOUNT=16 -DSOME_PATH="/path/to/file" firmware.firm

The `name` must be a valid Firmion [identifier](#identifiers).  The `value` is
optional and can be numeric or a quoted string.  Without a value, Firmion sets the
`const` to 1, with type `Integer`, following the GCC boolean-flag convention.

`-D` **overrides** any same-named `const` definition in the source.

Map output lists all `const` definitions including `-D` consts.

### Value Type Inference

Firmion knows or infers the type from the value string using the same rules as
source code for type inference.

| Example          | Value   | Type      | Description                                |
| ---------------- | ------- | --------- | ------------------------------------------ |
| `-DFLAG`         | 1       | `Integer` | Defaults to true (1).                      |
| `-DCOUNT=16`     | 16      | `Integer` | Plain decimal → `Integer`                  |
| `-DBASE=0x1000`  | 0x1000  | `U64`     | Hex/binary without suffix → implicit `U64` |
| `-DBASE=0x1000u` | 0x1000  | `U64`     | `u` suffix → explicit `U64`                |
| `-DOFFSET=0x40i` | 0x40    | `I64`     | `i` suffix → explicit `I64`                |
| `-DDELTA=-4`     | -4      | `I64`     | Negative decimal → implicit `I64`          |
| `-DMSG="Hello"`  | "Hello" | `String`  | Quoted string → `String`                   |

### Example

Define a base address at the command line:

    firmion -DBASE=0x0800_0000 firmware.firm -o firmware.bin

The source can reference `BASE` as an ordinary const:

    section entry { wr8 0x01; }
    section top   { set_addr BASE; wr entry; }
    output top;

---

# Firmion Language Reference

## Comments

Firmion supports C language line and block comments.

## Whitespace

Firmion supports lenient C language style whitespace rules.

## Semicolon Termination

Like C language, statements must be terminated with a trailing semicolon
character.

## Types

Firmion supports the following data types:

- U64: 64-bit unsigned values
- I64: 64-bit signed values
- Integer: 64-bit integers with flexible sign treatment
- String: UTF-8 string in double quotes

Firmion reports an error for under/overflow on arithmetic operations on U64, I64
and Integer types as described in [Arithmetic Operators](#arithmetic-operators).

## Identifiers

An identifier begins with a letter (A–Z, a–z) or an underscore (_), followed by
zero or more letters, digits (0–9), or underscores.  Identifiers are
case-sensitive.

### Reserved Identifiers

Firmion reserves certain identifiers and rejects their use as
[section](#section) names, const names, or label names at compile time.

Firmion also reserves two identifier *prefixes*.  Any user defined identifier
beginning with a reserved prefix triggers an error.

| Reserved Prefix | Reason                                                                        |
| --------------- | ----------------------------------------------------------------------------- |
| `wr` + digit    | Numeric write instructions (`wr8`, `wr16`, `wr32`, and future width variants) |
| `__`            | Leading double underscore names refer to builtin identifiers.                 |

Firmion also reserves the following *exact* keywords:

| Reserved Keyword | Reason / possible future use  |
| ---------------- | ----------------------------- |
| `import`         | Module inclusion              |
| `if`             | Conditional section inclusion |
| `else`           | Conditional section inclusion |
| `true`           | Boolean literal               |
| `false`          | Boolean literal               |
| `extern`         | External section references   |
| `let`            | Variable declarations         |
| `fill`           | Fill / pad byte ranges        |

Keyword reservation is case-sensitive.  `Fill` and `FILL` are valid identifiers;
`fill` is not.

---

## Literals

### Number Literals

Firmion supports number literals in decimal, hex (0x) and binary (0b) forms.
After the first digit, you can use '_' within number literals to help with
readability.

    assert 42 == 42;
    assert -42 == -42;
    assert 0x42 == 0x42;
    assert 0x42 == 66;
    assert 0x4_2 == 66;
    assert 0x42 == 6_6;

    assert 0b0 == 0;
    assert 0b01000010 == 0x42;
    assert 0b0100_0010 == 0x42;
    assert 0b101000010 == 0x142;
    assert 0b0000_0000_0100_0010 == 0x42;

The following table summarizes how Firmion determines the type of number literals.

| Example | Type    | Description                                                        |
| ------- | ------- | ------------------------------------------------------------------ |
| 4       | Integer | Simple decimal numbers are `Integer` type with flexible signedness |
| 4u      | U64     | Explicitly `U64`                                                   |
| 4i      | I64     | Explicitly `I64`                                                   |
| -4      | I64     | Negative numbers are `I64`                                         |
| 0x4     | U64     | Hex numbers are `U64` by default                                   |
| 0x4i    | I64     | Explicitly `I64` hex number                                        |
| 0b100   | U64     | Binary numbers are `U64` by default                                |

Firmion does not support negative hex or binary literals.

For convenience, the compiler casts the flexible `Integer` type to `U64` or
`I64` as needed.

    assert 42u == 42;  // U64 operates with Integer
    assert 42i == 42;  // I64 operates with Integer

Otherwise the types used in an expression must match.  For example:

    assert 42u == 42i; // mix unsigned and signed

Produces an error message:

    [ERR_137] Error: Input operand types do not match.  Left is 'U64', right is 'I64'
       ╭─[tests/integers_5.firm:2:12]
       │
     2 │     assert 42u == 42i; // mix unsigned and signed
       ·            ^^^    ^^^
    ───╯

Users can explicitly cast a number literal or expression to the required
signedness using the built-in `to_u64` to `to_i64` functions.  For example:

    assert -42 != to_i64(42);  // comparing signed to unsigned

The `to_u64` and `to_i64` functions **DO NOT** report an error if the runtime
value under/overflows the destination type.

    assert 0xFFFF_FFFF_FFFF_FFFF == to_u64(-1); // OK
    assert to_i64(0xFFFF_FFFF_FFFF_FFFF) == -1; // OK

### Number Magnitude

Decimal number literals accept an optional K/M/G magnitude suffix (case
sensitive) before the type suffix.

| Suffix | Multiplier         | Example | Value      |
| ------ | ------------------ | ------- | ---------- |
| `K`    | 1024               | `64K`   | 65536      |
| `M`    | 1024 × 1024        | `1M`    | 1048576    |
| `G`    | 1024 × 1024 × 1024 | `2G`    | 2147483648 |

Magnitude and type suffixes combine: `4Ku` is 4096 as a U64, `-1Ki` is -1024 as
an I64.

### True and False

Firmion considers a zero value false and all non-zero values true.

### Quoted Strings

Firmion allows utf-8 quoted strings with the following escape characters:

| Escape Character | UTF-8 Value | Name           |
| ---------------- | ----------- | -------------- |
| \\0              | 0x00        | Null           |
| \\t              | 0x09        | Horizontal Tab |
| \\n              | 0x0A        | Linefeed       |
| \\"              | 0x22        | Quotation Mark |

Newlines are Linux style, so "A\n" is a two byte string on all platforms.

## Arithmetic Operators

Firmion supports the following arithmetic operators with same relative precedence
as the Rust language.  Where applicable, Firmion checks for arithmetic
under/overflow.

| Precedence | Operator | Under/Overflow<br>Check? | Description                                  |
| ---------- | -------- | ------------------------ | -------------------------------------------- |
| Highest    | (   )    | n/a                      | Paren grouping                               |
|            | *   /    | yes                      | Multiply and divide                          |
|            | +   -    | yes                      | Add and subtract                             |
|            | &        | n/a                      | Bitwise-AND                                  |
|            | \|       | n/a                      | Bitwise-OR                                   |
|            | <<  >>   | no                       | Bitwise shift up and down                    |
|            | ==  !=   | n/a                      | Equal and non-equal                          |
|            | >=  <=   | n/a                      | Greater-than-or-equal and less-than-or-equal |
|            | &&       | n/a                      | Logical-AND                                  |
| Lowest     | \|\|     | n/a                      | Logical-OR                                   |

---

## addr

`addr( [identifier] ) -> U64`

When called with an identifier, returns the address of the identifier as a U64.
When called without an identifier, returns the current address.  See [Addresses
and Offsets](#addresses-and-offsets) for more information.

The following table shows the scoping rules for `addr`.  To summarize, Firmion
tracks **exactly one address value** per name.  An `addr(<name>)` command
retrieves that one value regardless of the scope of the caller.

| Command Form                  | Scope used to determine address                         |
| ----------------------------- | ------------------------------------------------------- |
| `addr()`                      | Scope of current section                                |
| `addr(<section name>)`        | Scope of parent section that contains the child section |
| `addr(<output section name>)` | Scope of the `output` section                           |
| `addr(<label name>)`          | Scope of the section that contains the label            |

Example:

    const BASE = 0x1000u;

    section fiz {
        assert addr() == BASE + 6;
        wrs "fiz";
        assert addr() == BASE + 9;
        assert addr(foo) == BASE;
    }

    section bar {
        assert addr() == BASE + 3;
        wrs "bar";
        assert addr() == BASE + 6;
        wr fiz;
        assert addr() == BASE + 9;
    }

    // top level section
    section foo {
        set_addr BASE;
        assert addr() == BASE;
        wrs "foo";
        assert addr() == BASE + 3;
        assert addr(fiz) == BASE + 6;
        wr bar;
        assert addr() == BASE + 9;
        assert addr(bar) == BASE + 3;
    }

    output foo;

---

## addr_offset

`addr_offset( [identifier] ) -> U64`

Returns the offset from the `output` or most recent `set_addr` anchor as a U64.
When called without an identifier, returns the current address offset.  When
called with an identifier, returns the address offset at the start of the named
section or label.

The offset resets to zero on each `set_addr` call.

The following table shows the scoping rules for `addr_offset`.  To summarize,
Firmion tracks **exactly one address offset value** per name.  An
`addr_offset(<name>)` command retrieves that one value regardless of the scope
of the caller.

| Command Form                         | Scope used to determine address                         |
| ------------------------------------ | ------------------------------------------------------- |
| `addr_offset()`                      | Scope of current section                                |
| `addr_offset(<section name>)`        | Scope of parent section that contains the child section |
| `addr_offset(<output section name>)` | Scope of the `output` section                           |
| `addr_offset(<label name>)`          | Scope of the section that contains the label            |

Example:

    const BASE = 0x1000u;

    section fiz {
        assert addr_offset() == 6;
        wrs "fiz";
        assert addr_offset() == 9;
        assert addr_offset(foo) == 0;
    }

    section bar {
        assert addr_offset() == 3;
        wrs "bar";
        assert addr_offset() == 6;
        wr fiz;
        assert addr_offset() == 9;
    }

    // top level section
    section foo {
        set_addr BASE;
        assert addr_offset() == 0;
        wrs "foo";
        assert addr_offset() == 3;
        assert addr_offset(fiz) == 6;
        wr bar;
        assert addr_offset() == 9;
        assert addr_offset(bar) == 3;
    }

    output foo;

---

## align

`align <expression> [, <pad byte value>];`

The align statement writes pad bytes into the current [section](#section) until
the absolute location counter reaches the specified alignment.  Align writes 0
as the default pad byte value, but the user may optionally specify a different
value.

Example:

    section foo {
        wrs "Hello";
        align 32;
        assert sizeof(foo) == 32;
        assert addr() == 32;
    }

    output foo;

---

## assert

`assert <expression>;`

The assert statement reports an error if the specified expression does not
evaluate to a true (non-zero) value.  Assert expressions provide a means of
error checking and do not affect the output file.

Example:

    section foo {
        assert 1;   // OK, non-zero is true
        assert -1;  // OK, non-zero is true
        assert 1 + 1 == 2;
    }

    output foo;

---

## const

`const <identifier> = <expr>;`

A const expression creates an immutable user defined identifier for a value. The
value can consist of a number or string literal, or an expression composed of
other constants and literals.  Const identifier names have global scope and must
be globally unique.  Const identifiers cannot conflict with any other global
identifiers such as [section](#section) names.

Example:

    const RAM_BASE = 0x8000_0000u;  // User defined unsigned constant.

    section foo {
        set_addr RAM_BASE;
        wr64 RAM_BASE;
        print "RAM base address is ", RAM_BASE, "\n";
    }

    output foo;

Const expressions support the full set of arithmetic, bitwise and comparison
operators. Comparison operators evaluate to 1 (true) or 0 (false) and are useful
for expressing relationships between constants:

    const FLASH_BASE = 0x0800_0000;
    const FLASH_SIZE = 0x0008_0000;
    const RAM_BASE   = 0x2000_0000;

    // Verify flash and RAM regions do not overlap
    const NO_OVERLAP = (FLASH_BASE + FLASH_SIZE) <= RAM_BASE;
    assert NO_OVERLAP;

A const value expression cannot depend on addresses, sizes, offsets or any other
dynamic aspect of the output file.  Firmion resolves all const values before
starting layout of the output.  For example:

    const RAM_BASE = 0x8000_0000;         // OK, just a 64b unsigned literal.
    const RAM_SIZE = 32768;               // OK, just a 64b integer literal.
    const RAM_END = RAM_BASE + RAM_SIZE;  // OK, const composed of other consts.

    section foo {
        wrs "Hello\n";
    }

    const RAM_USED = sizeof(foo);         // ERROR!  Const cannot depend on section properties.

    output foo;

### Deferred Assignment

`const` variables support deferred assignment.  This allows the user to declare
a `const` variable, then assign a value to the variable *exactly once* in later
code.  For example:

    const IO_START;
    ...
    IO_START = 0xF000_0000_0000_0000;

Deferred assignment is primarily useful in
[`if/else`](#ifelse) statements, which allow users to
conditionally determine the value to assign.

To provide errors and warnings, Firmion tracks the defined/undefined and
used/unused state of each variable.

---

## if/else

`if <expression> { ... } else { ... }`

Allows conditional execution of other statements. As described in [Output
Creation](#output-creation-phases), Firmion evaluates all `if/else` statements
before starting layout of the output.  Therefore, an `if/else` expression must
only depend on `const` variables and literal values.  In other words, `if/else`
statements must not depend on dynamic addresses, sizes, offsets or any other
layout dependent aspect of the output file.

Users must pre-declare `const` variables before conditionally assigning values
to them. For example:

    // Assume the user specified -DMEM_CONFIG="BIG" on the command line.

    // Pre-declare variables prior to conditional assignment in an if/else.
    // Firmion strictly tracks variable definitions to prevent use of
    // uninitialized variables.
    const FLASH_SIZE;
    const RAM_SIZE;

    print "Memory configuration is ", MEM_CONFIG, "\n";
    if MEM_CONFIG == "BIG" {
        FLASH_SIZE = 0x8_0000;
        RAM_SIZE = 0x80_0000;
        include "big_config.firm";
    } else {
        if MEM_CONFIG == "MEDIUM" {
            FLASH_SIZE = 0x4_0000;
            RAM_SIZE = 0x40_0000;
            include "medium_config.firm";
        } else {
            if MEM_CONFIG == "SMALL" {
                FLASH_SIZE = 0x2_0000;
                RAM_SIZE = 0x20_0000;
                include "small_config.firm";
            } else {
                print "Invalid configuration. MEM_CONFIG must be BIG, MEDIUM, or SMALL.\n";
                assert(0);  // Halt execution
            }
        }
    }

If the taken path in an `if/else` statement does not assign a value to a
predeclared `const` variable, then Firmion reports an error if any later program
statement uses that variable.

For compactness, user's may omit braces around an `else/if` block.  For example:

    if MEM_CONFIG == "BIG" { include "big_config.firm"; }
    else if MEM_CONFIG == "MEDIUM" { include "medium_config.firm"; }
    else if MEM_CONFIG == "SMALL" { include "small_config.firm"; }
    else { assert(0); }

---

## include

`include "<file>";`

Includes another Firmion source file.  Firmion processes the included file as if it
were part of the current file.  For example, the included file can define
sections, labels, constants and nested include files.

An included file may contain an output statement.  Firmion will enforce that the
entire program after include file resolution contains only one output statement.
See the [`output`
statement](#output) for more
information.

The default path for an included file is the directory of the source file that
contains the include statement.  For example, if `main.firm` is in
`/home/user/project/` and contains `include "sections.firm"`, then Firmion will
look for `/home/user/project/sections.firm`.

Include files starting with a `/` are absolute paths.  Likewise, Firmion supports
relative paths such as `../`.

All paths use Linux style forward slashes.

Example:

    // file: main.firm
    include "../constants.firm";
    include "sections.firm";

    output main_rom;

    // file: ../constants.firm
    const RAM_BASE = 0x8000_0000u;

    // file: sections.firm
    section main_rom {
        set_addr 0x1000;
        wrs "Hello\n";
    }

---

## Labels

`<identifier>:`

Labels assign an identifier to a specific location in the output file.  Programs
can then refer to the location of the label by name.  Labels names have global
scope and label names must be globally unique.  Multiple different labels can
refer to the same location.

Labels have the form `<label identifier>:` and can prefix most statement types.

For example:

    section foo {
        set_addr 0x1000;
        // assign the label 'lab1' to the current location
        lab1: wrs "Wow!";
        // assign the label 'lab2' to the current location
        lab2:
        assert addr(lab1) == 0x1000;
        assert addr(lab2) == 0x1004;
        assert addr(lab3) == 0x1004;
        // yet another label, same location as 'lab2'
        lab3:
    }

    output foo;
---

## obj

`obj <obj name> { ... }`

An `obj` statement assigns a name to one or more sections in one or more
external object or executable files.  The *section* in this case is not a
Firmion `section`, but a linker section such as ".text" or ".rodata" created by
an external compiler toolchain, e.g.
[gcc](https://en.wikipedia.org/wiki/GNU_toolchain).

By default, Firmion supports only the
[ELF](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) format.
However, using compile-time feature flags, users can enable support for any
object file format supported by the Rust
[object](https://crates.io/crates/object) crate and rebuild Firmion from source.

> [!NOTE]
> On Linux systems, use `objdump -h <filename>` to list all the
> sections in a compatible binary file.

For example:

    obj runtime_code {
        file = "/absolute/path/to/file";
        section = ".text";
    }

    obj runtime_rodata {
        file = "relative/path/file1";
        section = ".rodata";
    }

    section main {
        wr runtime_code;
        wr runtime_rodata;
    }

    output main;

Users can glob multiple files and/or sections into a single `obj` using a syntax
similar to [GNU `ld` wildcard
patterns](https://sourceware.org/binutils/docs/ld/Input-Section-Wildcards.html).
For example:

    // Gather all .text sections in all object files
    obj all_text {
        file = "/path/to/*.o";  // All object files
        section = ".text*";     // All text sections
    }

Users can also specify multiple match patterns as a comma separated list in
`[]` and can exclude files or sections.  For example:

    obj all_text {
        file = ["/path/to/*.o", "/path/to/*.elf"];  // All .o and .elf files
        section = ".text*";     // All text sections
        // Exclude any section with 'debug' in the name and
        // also exclude .text.foo1 and .text.foo2
        section_exclude = ["*debug*", ".text.foo[12]"];
    }

### Obj Properties

Obj definitions support the following properties:

- `file` Pattern or pattern list to match to the object file(s) (required)
- `section` Pattern or pattern list to match to the name of the section(s) in
  the object file (required)
- `file_exclude` Pattern or pattern list to match to exclude files in the `file`
  pattern
- `section_exclude` Pattern or pattern list to match to exclude sections in the
  `section` pattern

#### Obj Property `file`

Pattern or pattern list to match the path to the object file(s).  Paths can be
relative to the current directory or absolute.  On both Linux and Windows, path
strings use forward-slashes `/` to denote directories.

#### Obj Property `file_exclude`

Pattern or pattern list to match to *exclude* object file(s) from the set
matching the `file` pattern.

#### Obj Property `section`

Pattern or pattern list to match the name of the linker section in the object
file(s).

#### Obj Property `section_exclude`

Pattern or pattern list to match to *exclude* sections from the set matching the
`section` pattern.

### Pattern Matching Syntax

Firmion support the following special characters for file globbing.  Firmion
silently allows an empty (no files and/or no sections) glob.

| Pattern     | Description                                     |
| ----------- | ----------------------------------------------- |
| `*`         | Match 0 or more characters                      |
| `?`         | Match a single character                        |
| `[<chars>]` | Match any single character in the specified set |
| `\*`        | Match a literal `*` character                   |
| `\?`        | Match a literal `?` character                   |
| `\[`        | Match a literal `[` character                   |
| `\]`        | Match a literal `]` character                   |
| `\"`        | Match a literal `"` character                   |
| `\\`        | Match a literal `\` character                   |

Users may also use GNU `ld` style sorting commands in a pattern.  These commands
appear inside the quoted expression string for the pattern and may be nested.
When globbing files, Firmion by default sorts the resulting file list by name.



| Sort Command         | Allowed in<br>File Match | Allowed in<br>Section Match | Description                                       |
| -------------------- | ------------------------ | --------------------------- | ------------------------------------------------- |
| SORT                 | Yes                      | Yes                         | Alias for SORT_BY_NAME                            |
| SORT_BY_NAME         | Yes                      | Yes                         | Sorts the match result by name                    |
| SORT_BY_ALIGNMENT    | No                       | Yes                         | Sorts the match result by alignment size          |
| SORT_BY_INIT_PIORITY | No                       | Yes                         | Sorts the match result by initialization priority |
| REVERSE              | Yes                      | Yes                         | Reverses the sort order                           |

Examples:

    obj runtime_code {
        file = [ "main.elf", "utils.elf", "drivers/*.elf" ];
        section = "SORT(.text*)";
    }

    // Place sorted init sections first, then remaining text
    obj boot_code {
        file = "*.o";
        section = [ "SORT(.init.*)", ".text*" ];
    }

    obj backwards {
        file = "thing.o";
        section = "REVERSE(SORT(.text*))";
    }

### Obj Size

Users can query the size of an `obj` with `sizeof`.  For example:

    obj runtime_rodata {
        file = "*.elf";
        section = ".rodata*";
    }

    section foo {
        print "The size of read-only data is ", sizeof(runtime_rodata), " bytes\n";
        wr64 sizeof(runtime_rodata);
        wr runtime_rodata;
    }

### Obj LMA and VMA

Some file formats, notably ELF, define a *load memory address* (LMA) for a
section.  The LMA specifies where a system stores a section *at rest*.  The
*virtual memory address* (VMA) specifies the *runtime* address of a section.
These addresses differ when a system copies the section before use, e.g. copies
program code from slow FLASH memory (at the *LMA*) to fast SRAM (at the *VMA*)
before execution.

Users can query the ELF LMA and VMA of an `obj` using the [`obj_lma`](#obj_lma) and
[`obj_vma`](#obj_vma) commands.

### Firmion `addr` vs LMA and VMA

Users fully control their output files and can use Firmion's address support
([addr](#addr), [set_addr](#set_addr), [region](#region), etc) as they see fit.
For most systems however, Firmion's `addr` plays the same role as the ELF LMA. The
Firmion `addr` value is usually the section's *storage* address as might be used
by a FLASH update utility.  Any subsequent copy at runtime is typically outside
the scope of Firmion.

When writing an `obj` to the output, Firmion does *not* validate that a
section's LMA matches the associated [`addr`](#addr) value.  This allows `obj`
placement in a simple staging area of the image file, presumably copied to the
actual LMA by a firmware update utility.  Users are free to keep the
[`addr`](#addr) value in sync with LMA and can use [`assert`](#assert)
statements for validation.  The section [Handling Objs with Disjoint
LMAs](#handling-objs-with-disjoint-lmas) shows an example of synchronizing `addr`
and LMA.

### Obj Properties and Section Globbing

[Glob patterns](#pattern-matching-syntax) group multiple linker sections into a
single `obj`, aka a `glob obj`.  Firmion packs sections contiguously, adding pad
bytes before each section (after the first) as needed to meet each section's
individual alignment requirement. A `glob obj` has the following properties:

- [`obj_lma`](#obj_lma) returns the LMA of the *first* section.
- [`obj_vma`](#obj_vma) returns the VMA of the *first* section.
- [`obj_align`](#obj_align) returns the *largest* alignment of any section in
  the `obj`.

### Glob Obj LMA Validation

Starting from the first section's LMA, Firmion validates the LMA of each
subsequent section.  If an LMA is not *contiguous* with the preceding section,
Firmion reports an error.  Firmion bypasses validation for sections with LMA=0.
The example below groups executable sections with disjoint LMAs which results in
an error.

    // .text in boot.elf has LMA = 0x0000_0000
    // .text in app.elf has LMA = 0x1000_0000
    obj combined {
        file = [ "boot.elf", "app.elf" ];
        section = ".text*";
    }
    // ERROR! Users cannot glob sections with disjoint LMAs into one obj

Users must place linker sections with disjoint LMAs into different `obj`s.

### Handling Objs With Disjoint LMAs

The output image, possibly in cooperation with a firmware update utility, must
save each `obj` and their contained sections at the required LMA.  Some
platforms use firmware images that define a single loadable address space.
These images map firmware file offset 1:1 with the destination address space.
In this case, the user can simply pad to achieve the correct LMA for each obj.
For example:

    // Output firmware image is 128K and contains
    // both boot and app executables.

    // .text in boot.elf has LMA = 0x0000_0000
    obj boot {
        file = "boot.elf";
        section = ".text*";
    }

    // .text in app.elf has LMA = 0x001_0000 (64k)
    obj app {
        file = "app.elf";
        section = ".text*";
    }

    section main {
        assert obj_lma(boot) == 0;     // We expect to start at addr = 0
        wr boot;
        pad_addr_offset obj_lma(app);   // We're 1:1, so just pad to app's LMA
        assert addr() == obj_lma(app);
        wr app;
    }

    output main;

Many platforms, such as the [ESP32 example](#esp32-s3), do not simply map
firmware images 1:1 with load address.  In these cases, an onboard FLASH update
utility copies disjoint sections from the firmware image to the actual platform
LMA.

---

## obj_align

`obj_align(<obj>) -> U64`

Returns the *alignment* of the specified [obj](#obj) as a U64.  The external
object file defines this value as set by a compiler toolchain.

For example:

    obj runtime_rodata {
        file = "/path/to/exe";
        section = ".rodata";
    }

    section foo {
        align obj_align(runtime_rodata);
        wr runtime_rodata;
    }

For a [`glob obj`](#obj-properties-and-section-globbing), `obj_align` returns the
largest alignment of any section in the `obj`.

---

## obj_lma

`obj_lma(<obj>) -> U64`

Returns the *load memory address* (LMA) of the specified [obj](#obj) as a U64.
The external object file defines this value as set by a compiler toolchain.  See
[LMA and VMA](#obj-lma-and-vma) for more information.

For example:

    obj runtime_rodata {
        file = "/path/to/exe";
        section = ".rodata";
    }

    section foo {
        print "The load address is ", obj_lma(runtime_rodata), "\n";
        // Keep the object file load address as-is.
        set_addr(obj_lma(runtime_rodata));
        wr runtime_rodata;
    }

Some non-ELF file formats do not support an LMA different from a VMA.  If the
user sets feature flags to enable additional object file formats and calls
`obj_lma` on an unsupported format, then Firmion simply returns the obj's VMA
value.  In this way, a user can consistently use `obj_lma` for all formats.

For a [`glob obj`](#obj-properties-and-section-globbing), `obj_lma` returns the
LMA of the first section in the `obj`.

---

## obj_vma

`obj_vma(<obj>) -> U64`

Returns the *virtual memory address* (VMA) of the specified [obj](#obj) as a
U64.  The external object file defines this value as set by a compiler
toolchain.  See [LMA and VMA](#obj-lma-and-vma) for more information.

For example:

    obj runtime_rodata {
        file = "/path/to/exe";
        section = ".rodata";
    }

    section foo {
        set_addr(obj_lma(runtime_rodata));
        // Our runtime doesn't support relocation.
        assert obj_vma(runtime_rodata) == obj_lma(runtime_rodata);
        wr runtime_rodata;
    }

For a [`glob obj`](#obj-properties-and-section-globbing), `obj_vma` returns the
VMA of the first section in the `obj`.

## output

`output <section identifier>;`

An output statement specifies the top [section](#section) to write to the output
file. Use `set_addr` inside the [section](#section) to control the absolute
starting address, or place the top [section](#section) in region with a start
address.

**A Firmion program must have exactly one output statement.**

An `include` file may contain an output statement.  Firmion will enforce that the
entire program after include file resolution contains only one output statement.

---

## print

`print <expression> [, <expression>, ...];`

The print statement evaluates the comma separated list of expressions and prints
them to the console.  For expressions, print displays unsigned values in hex and
signed values in decimal.  If needed, the `to_u64` and `to_i64` functions can
control the output style.

Firmion executes a given print statement for each instance found in the output
file.  In other words, a print statement in a [section](#section) written
multiple times will execute multiple times in output order.

Example:

    const BASE = 0x1000;

    section bar {
        print "Section 'bar' starts at ", addr(), "\n";
        wrs "bar";
    }

    // top level section
    section foo {
        set_addr BASE;
        print "Output spans address range ", BASE, "-", BASE + sizeof(foo),
              " (", to_i64(sizeof(foo)), " bytes)\n";
        wrs "foo";
        wr bar;
        wr bar;
        wr bar;
    }

    output foo;

Results in the following console output:

    Output spans address range 0x1000-0x100C (12 bytes)
    Section 'bar' starts at 0x1003
    Section 'bar' starts at 0x1006
    Section 'bar' starts at 0x1009

---

## region

`region <identifier> { ... }`

A `region` declares the name and *static* properties of an address range.
Regions provide a way to decouple memory placement and top-down layout control
from the [section](#section) content being placed.  Unlike sections, regions are
stateless and do not track dynamic information during layout.

Users place *exactly one* [section](#section) `in` a region.  We refer to this
[section](#section) as the *bound section* of the region.  The bound
[section](#section) is a normal section with the following extra behaviors:

- The region sets the starting address of the bound section.
- The region caps the size of the bound section.

For example:

    // Define the properties of the FLASH memory region
    region FLASH {
        addr = 0xF000_0000;
        size = 1M;
        default_pad_byte = 0xFF;
    }

    // Define the properties of the EEPROM memory region
    region EEPROM {
        addr = 0xFF00_0000;
        size = 64K;
    }

    // Flash sections
    section boot { ... }
    section flash_code { ... }
    section flash_data { ... }

    // EEPROM sections
    section eeprom_data1 { ... }
    section eeprom_data2 { ... }

    // FLASH_TOP is the bound section in the FLASH region
    section FLASH_TOP in FLASH {
        // Starts at address 0xF000_0000
        wr boot;
        wr flash_code;
        wr flash_data;
    }

    section EEPROM_TOP in EEPROM {
        assert addr() == 0xFF00_0000;
        wr runtime_code;
        wr runtime_data;
    }

    // The output file contains the image for FLASH and EEPROM regions.
    // This section is not a bound section of a region and behaves
    // like any other section.
    section FIRMWARE_UPDATE_FILE {
        wr file_offset(FLASH_TOP);    // Offset to the new FLASH image
        wr file_offset(EEPROM_TOP);   // Offset to the EEPROM image
        wr FLASH_TOP;                 // FLASH image
        wr EEPROM_TOP;                // EEPROM image
    }

    output FIRMWARE_UPDATE_FILE;  // Write the output

### Region Properties

Regions support the following properties:

- `addr` Starting address (required)
- `size` Size in bytes (required)
- `default_pad_byte` Default pad byte for the region (Default = 0xFF) (optional)

#### Region Property `addr`

The `addr` property defines the region's absolute starting address.  The
region's bound [section](#section) starts at this address.  Users can query the `addr`
property of a region with `addr(<region name>)`.

#### Region Property `size`

Specifies the size of the region in bytes.  Firmion reports an error if the
size of the bound [section](#section) exceeds this value.

Users can query the `size` property of a region with `sizeof(<region name>)`.

The size value accepts a [K/M/G magnitude suffix](#number-magnitude).

#### Region Proper `default_pad_byte`

Optionally specifies the default pad byte value for the region.  The default
value is 0xFF.

### Region Boundary Enforcement

Regions provide automatic size and boundary checking for all operations in the
region.  In practical terms this means:

- Write commands in a region cannot extend outside the region
- Address manipulation in a region cannot result in an address outside the
  region
- Offset manipulations in a region cannot result in a offset outside the
  region.

For example, the following `wr32` command would extend outside the region by one
byte, resulting in an error:

    region LITTLE_ROM {
        addr = 0;
        size = 7;
    }

    section data in LITTLE_ROM {
        // occupies bytes 0-3
        wr32 0x12345678;
        // Occupies bytes 4-7, which extends 1 byte outside the region
        wr32 0x87654321;  // ERROR!
    }

Of course, region enforcement occurs not just in the region's bound section,
but in any reachable section.  For example:

    region LITTLE_ROM {
        addr = 0;
        size = 7;
    }

    section nested_stuff {
        pad_sec_offset 6;  // pad to last byte of region
        wr more_nested;
    }

    section more_nested {
        wr std::crc32c(more_nested);  // 4 bytes of output
    }

    section data in LITTLE_ROM {
        wr nested_stuff;  // ERROR!  Data written outside of region
    }

The [set_addr](#set_addr) command and any offset manipulation commands are also
constrained to fit in the region.  For example:

    region FLASH {
        addr = 0xF000_0000;
        size = 1M;
    }

    section foo in FLASH {
        assert addr() == 0xF000_0000;  // Start of FLASH region
        set_addr 0xF000_1000;          // OK, inside the region
        wrs "Inside region!";
        set_addr 0xA000_0000;          // ERROR, outside the region
        wrs "Outside region!";
    }

    output foo;

### Nested Regions

Users can freely nest sections in different regions into each other.  However,
Firmion allows write operations only in the address range intersection permitted
by *all* the parent regions.  For example:

    region READ_ONLY {
        addr = 0xF000_0000;
        size = 0x1000_0000;
    }

    region FLASH {
        addr = 0xF100_0000;
        size = 64K;
    }

    section flash_data in FLASH {
        assert addr() == 0xF100_0000;
        ...
    }

    section ro_data in READ_ONLY {
        assert addr() == 0xF000_0000;
        // OK, region FLASH is a subset of READ_ONLY.
        // Note that the FLASH region anchors the starting address
        // at 0xF100_0000.  This creates a logical (unpadded) address gap
        // in the ro_data section between 0xF000_0000 and 0xF100_0000.
        wr flash_data;
    }

    output ro_data;

### Partially Overlapping Nested Regions

For completeness, the region of a nested [section](#section) need not be a
proper subset of the parent region. Firmion still enforces the constraints of
*all* parent sections as follows:

- Any address written by the child [section](#section) must lie in the
  intersection of *all* parent regions.
- The starting address of a nested [section](#section) must fit the address
  range allowed by all parent regions.

### Sections in Regions are Single Use

Placing a [section](#section) in a region forces the starting address of the
[section](#section) to the region's `addr` value.  Writing this
[section](#section) more than once results in an address address conflict with
the previous instance of the section.

---

## sec_offset

`sec_offset( [identifier] ) -> U64`

When called with an identifier, returns the unsigned 64-bit offset of the
identifier from the start of the [section](#section) that contains the
identifier.  When called without an identifier, returns the offset from the
start of the current section.

Example:

    section fiz {
        assert sec_offset() == 0;
        wrs "fiz";
        assert sec_offset() == 3;
    }

    section bar {
        assert sec_offset() == 0;
        wrs "bar";
        assert sec_offset() == 3;
        wr fiz;
        assert sec_offset() == 6;
        assert sec_offset(fiz) == 3;
    }

    // top level section
    section foo {
        assert sec_offset() == 0;
        wrs "foo";
        assert sec_offset() == 3;
        wr bar;
        assert sec_offset() == 9;
    }

    output foo;

When a [section](#section) offset specifies an identifier, the identifier must be in the
scope of the current section.  For example:

    section fiz {
        wrs "fiz";
    }

    section bar {
        wr fiz;
        assert sec_offset(fiz) == 0; // OK fiz in scope in section bar
    }

    section foo {
        wr bar;
        assert sec_offset(bar) == 0; // OK, bar is local in this section
        assert sec_offset(fiz) == 0; // ERROR, fiz is out of scope in section foo
    }

    output foo;

---

## section

`section <name> [in <region>] { ... }`

A section is a named, reusable block of content.  Sections are the primary
building block of a Firmion program.  Each section defines a sequence of bytes,
built up from write statements and padding operations such as `align`.
Sections may also contain labels, assertions, print statements and so on.
Sections may write other sections into themselves so long as the nesting does
not create a cycle.

Section names must be valid [identifiers](#identifiers), must be globally
unique, and must not conflict with const names, label names, region name, or
[reserved identifiers](#reserved-identifiers).

Sections have their own section-relative location counter which resets to zero
at the start of each section.  Sections can read and advance the section
location counter with [`sec_offset()`](#sec_offset) and
[`pad_sec_offset()`](#pad_sec_offset) statements
respectively.

The root section named in the [`output`](#output) statement is the only section
Firmion writes to the output file.  Other sections can be directly or indirectly
included via [`wr`](#wr) statements from the output section.  Unreachable
sections produce a warning.

### Sections In Regions

To help guide layout, users can place a exactly one section `in` a
[region](#region) with `in <region name>` after the section name.  We call a
section placed in a region as the *bound section* of the region.

Example:

    section magic {
        wrs "FIRM";           // 4-byte magic number
        wr8 0x01;             // version
        assert sec_offset() == 5;    // Section location counter should be 5
    }

    section body {
        wr8 0xAA, 16;         // 16 bytes of payload
    }

    section image {
        wr magic;
        align 256;            // Body should start on 256 byte boundary
        wr body;
        assert sizeof(image) == 272;  // 256 + 16
    }

    output image;

---

## set_addr

`set_addr <expression>;`

The `set_addr` command forces the current address to the specified value and
resets the current `addr_offset` to zero.  These changes happen within the scope
of the containing section.  Child sections inherit the parent section's `addr`
and `addr_offset` values.

Using `set_addr` *does not* change the value of the [section](#section) offset nor file
offset.  A `set_addr` command *does not* add pad bytes to the output.

The `set_addr` command may move the address forward or backwards.  However,
Firmion tracks every output byte by address and reports an error if a program
tries to write to the same address more than once.

Example:

    section foo {
        wr8 1;
        wr8 2;
        wr8 3;
        wr8 4;
        wr8 5;
        set_addr 16;
        assert addr() == 16;
        assert addr_offset() == 0;   // set_addr resets addr_offset
        assert file_offset() == 5;  // set_addr does not pad
        assert sec_offset() == 5;
        wr8 0xAA, 3;
        assert addr_offset() == 3;
        assert file_offset() == 8;
        assert sec_offset() == 8;
        pad_sec_offset 24, 0xFF;     // Adds 24 - 8 = 16 pad bytes
        assert addr() == 35;         // 19 + 16 = 35
        assert addr_offset() == 19;  // 3 + 16 = 19
        assert file_offset() == 24;  // 8 + 16 = 24
        assert sec_offset() == 24;   // 8 + 16 = 24
    }

    output foo;

When used in a [section](#section) in a [region](#region), Firmion reports an error if the `set_addr` command sets the address outside of a region.

---

## pad_addr_offset

`pad_addr_offset <expression> [, <pad byte value>];`

Pads the output until `addr_offset` reaches the specified value.  Users may
specify an optional pad byte value or use the default value of 0.

If the specified value is less than the current `addr_offset`, Firmion reports an
error.

`pad_addr_offset` is most useful after a `set_addr` call, because `set_addr`
resets `addr_offset` to zero.  This lets users pad to a size relative to their
chosen address anchor without knowing what the surrounding section's
`sec_offset` happens to be.

Example:

    const BASE = 0x1000;

    section header {
        wrs "FIRM";           // 4-byte magic number
        wr8 0x01;             // version byte
    }                         // addr_offset == 5 on exit

    section body {
        set_addr BASE;
        wr header;
        // Relocate body to its target load address.
        // addr_offset resets to 0.
        set_addr 0xF000;
        wr8 0xAA, 3;          // 3 bytes of payload
        // Pad to 0x20 bytes from the 0xF000 anchor.
        pad_addr_offset 0x20;
        assert addr() == 0xF020;
        assert addr_offset() == 0x20;
        assert sec_offset() == 0x25;  // 5 (header) + 3 (payload) + 29 (pad) = 0x25
    }

    output body;

---

## pad_file_offset

`pad_file_offset <expression> [, <pad byte value>];`

The pad_file_offset command pads the output file until the *file offset* reaches
the specified value.  Users may specify an optional pad byte value or use the
default value of 0.

If the specified offset is less the current offset, Firmion reports an error.

`pad_file_offset` is most useful when a [section](#section) is written inside a parent
section, because `sec_offset` resets to zero at the start of each child section
while `file_offset` continues from the parent's position.  This lets a child
section pad to an absolute file position regardless of where the parent places
it.

Example:

    // A firmware container: an 8-byte header at file offset 0, followed by a
    // payload that must start at file offset 512 for bootloader compatibility.

    section header {
        wrs "FIRM";       // 4-byte magic
        wr32 0x00000001;  // version
    }

    section payload {
        // firmware writes header first (8 bytes), so payload opens at
        // file_offset 8.  Pad to the protocol-required file position 512.
        pad_file_offset 512, 0xFF;
        assert file_offset() == 512; // absolute position in the output file
        assert sec_offset() == 504;  // sec_offset starts from 0 inside payload
        wrs "PAYLOAD";               // 7 bytes of payload data
        assert file_offset() == 519;
        assert sec_offset() == 511;
    }

    section firmware {
        wr header;
        wr payload;
    }

    output firmware;

---

## pad_sec_offset

`pad_sec_offset <expression> [, <pad byte value>];`

The pad_sec_offset command pads the current [section](#section) until the *section offset*
reaches the specified value.  Users may specify an optional pad byte value or
use the default value of 0.

If the specified offset is less the current offset, Firmion reports an error.

Example:

    section foo {
        wr8 1;
        wr8 2;
        wr8 3;
        wr8 4;
        wr8 5;
        pad_sec_offset 16;
        assert addr() == 16;
        assert file_offset() == 16;
        assert sec_offset() == 16;
        wr8 0xAA, 3;
        pad_sec_offset 24, 0xFF;
        assert addr() == 24;
        assert file_offset() == 24;
        assert sec_offset() == 24;
        pad_sec_offset 24, 0xEE; // should do Nothing
        wr8 0xAA, 3;
        pad_sec_offset 27, 0x33; // should do nothing
        pad_sec_offset 28, 0x77; // should pad to 28
        assert sizeof(foo) == 28;
    }

    output foo;

---

## sizeof

`sizeof( <identifier> ) -> U64`

Returns the size in bytes of the specified identifier.

Example:

    section empty_one {}
    section foo {
        wrs "Wow!";
        wr empty_one;
        assert sizeof(empty_one) == 0;
        assert sizeof(foo) == 4;
    }

    output foo;
---

When called with an [extension](#firmion-extensions) identifier, `sizeof` returns the size of the
extension's output.  For example:

    print "CRC size=", sizeof(std::crc32c);  // returns "CRC size=4"

When called with a [region](#region) identifier, `sizeof` returns the fixed size of the region regardless of whether the user's program writes any data in the region.

    region FLASH { ...; size = 8K; ... }
    ...
    print "FLASH size=", sizeof(FLASH);  // returns "FLASH size=8192"

When called with a [section](#section) identifier, `sizeof` returns the size of the [section](#section) *in the file*.  Therefore, this size does not take into account operations that do not write data nor pad bytes.  For example, address jumps, e.g. by using [set_addr](#set_addr) do not change the sizeof() result for a section.

    section foo {
        set_addr 0;
        wrs "Hello\n";
        // Address jumps by 0x1000, but no data nor pads written, so
        // no effect on sizeof(foo).
        set_addr 0x1000;
        wrs "World\n";
        assert sizeof(foo) == 12;
    }
---

## to_i64

`to_i64( <expression> ) -> I64`

Converts the specified expression to the I64 type without regard to
under/overflow.

Example:

    section foo {
        assert to_i64(0xFFFF_FFFF_FFFF_FFFF) == -1;
        assert to_i64(42u) == 42;
        assert to_i64(42u) == 42i;
        assert to_i64(42) == 42i;
    }

    output foo;

---

## to_u64

`to_u64( <expression> ) -> U64`

Converts the specified expression to the U64 type without regard to
under/overflow.

Example:

    section foo {
        assert 0xFFFF_FFFF_FFFF_FFFF == to_u64(-1);
        assert to_u64(42i) == 42;
        assert to_u64(42i) == 42u;
        assert to_u64(42) == 42u;
    }

    output foo;

---

## trace

`trace <expression> [, <expression>, ...];`

The `trace` command provides debug output to help diagnose errors before Firmion
is able to internally execute a program.  Trace is especially useful for errors
reported during layout phase operations before Firmion determines the size and
location of everything in the output file.  During this time, Firmion is
actively *cooking* the output, so layout dependent values revealed by `trace`
may change as iteration proceeds.

> [!NOTE]
> Firmion suppresses `trace` output unless the user specifies at least one `-v`
> verbosity level on the command line.

The `trace` command has the same form and capability as the [`print`](#print)
command. However, `trace` executes on every internal iteration pass as Firmion
tries to stabilize the output's layout.

> [!NOTE]
> Firmion does not specify the order of execution of `trace` relative to other
> statements in the program.

> [!NOTE]
> As shown by a `trace` command, layout dependent values such as
> addresses and sizes may change as Firmion iterates.  Use [`print`](#print) to see
> stabilized values.

For example:

    trace "Start!\n";
    section B {
        trace "Size of B is ", sizeof(B), "\n";
        wr A;
        wr8 0xBB;
        trace "B1\n";
    }

    section A {
        wr8 0xAA;
        trace "A1\n";
    }

    trace "Top1\n";
    output B;
    trace "Finish!\n";

The program above *might* produce an output like the following, the caveat being
that Firmion purposely does not rigorously define trace output:

    [Trace-1] Start!
    [Trace-1] Top1
    [Trace-1] Size of B is 0x0
    [Trace-1] A1
    [Trace-1] B1
    [Trace-1] Finish!
    [Trace-2] Start!
    [Trace-2] Top1
    [Trace-2] Size of B is 0x2
    [Trace-2] A1
    [Trace-2] B1
    [Trace-2] Finish!
    [Trace-2] Start!
    [Trace-2] Top1
    [Trace-2] Size of B is 0x2
    [Trace-2] A1
    [Trace-2] B1
    [Trace-2] Finish!

The number in Trace-*n* is the internal iteration count.  In this example, Firmion
required two iterations to stabilize the output.

## Transient Values in `trace` Output

Because the `trace` command provides a peek into Firmion's internal workings,
displayed values may appear erroneous.  In particular, transient values (often
equal to 0) can cause arithmetic expressions to also return bogus results.
During image generation, Firmion deals with bogus values by suppressing certain
types of errors until the image stabilizes.  When suppressing an error, Firmion
substitutes 0 for the bogus value.  As Firmion iterates, "good" values propagate
and eventually eliminate these suppressed errors.

In summary, treat `trace` values as hints.  As iteration continues, `trace`
values should converge on the actual value, i.e. what [`print`](#print) would
display.

---

## wr

The `wr` command has two forms.  The first form writes the contents of another
section into the current section. The second `wr` form invokes an extension and
writes the output into the current section.

### wr section

`wr <section identifier>;`

Firmion adds the specified in [section](#section) to the current [section](#section) at the current
section offset.

### wr extension

`wr <namespace>::<extension_name>(<arg1>, <arg2>, ...);`

Evaluates the specified extension call and writes the result to the output.  The
extension's `.size()` method specifies the size of the result.  See [Firmion
Extensions](#firmion-extensions) for more information.

### Example

Using `wr`, you can build complex outputs by composing smaller, modular sections
together.

Example:

    section header {
        wrs "FILE";   // Write a string.
        wr8 0x01;     // Write a byte.
    }

    section data {
        wrs "DATA";
        wr8 0xFF, 4;
    }

    // Compose the top-level section
    section my_firmware {
        wr header;
        wr data;
        // Use an extension to append a CRC to a section.
        // Extensions can refer to their containing section.
        wr std::crc32c(my_firmware);
    }

    output my_firmware;

---

## wr8 to wr64

Little-endian:

`wr8 <expression> [, <expression>];`
`wr16 <expression> [, <expression>];`
`wr24 <expression> [, <expression>];`
`wr32 <expression> [, <expression>];`
`wr40 <expression> [, <expression>];`
`wr48 <expression> [, <expression>];`
`wr56 <expression> [, <expression>];`
`wr64 <expression> [, <expression>];`

Big-endian:

`wrbe8 <expression> [, <expression>];`
`wrbe16 <expression> [, <expression>];`
`wrbe24 <expression> [, <expression>];`
`wrbe32 <expression> [, <expression>];`
`wrbe40 <expression> [, <expression>];`
`wrbe48 <expression> [, <expression>];`
`wrbe56 <expression> [, <expression>];`
`wrbe64 <expression> [, <expression>];`

Evaluates the first expression and writes the result as a little-endian
value to the output file, or as a big-endian value for the `be` form.  The
optional second expression specifies the repetition count.

> [!IMPORTANT] Firmion silently truncates the upper bits of the expression to fit the specified width.

Example:

    // Test expressions in wrx; addr(foo) == 10 as set by region TOP
    region TOP { addr = 10; size = 64K; }
    section foo in TOP {
        wr8  (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 0 + 10 + 36  = 49
        wr16 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 1 + 10 + 36  = 50 00
        wr24 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 3 + 10 + 36  = 52 00 00
        wr32 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 6 + 10 + 36  = 55 00 00 00
        wr40 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 10 + 10 + 36 = 59 00 00 00 00
        wr48 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 15 + 10 + 36 = 64 00 00 00 00 00
        wr56 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 21 + 10 + 36 = 70 00 00 00 00 00 00
        wr64 (1 + 2) + file_offset() + addr(foo) + sizeof(foo); // 3 + 28 + 10 + 36 = 77 00 00 00 00 00 00 00
        assert sizeof(foo) == 36;
    }

    output foo;

Another example using the optional repetition expression.

    section foo {
        wr32 0x12345678, 10; // write 0x12345678 10 times to the output file.
        wr8 0, addr() % 4096; // write zero enough times to align to 4KB boundary.
    }

---

## `wrf "<quoted file path>";`

Write the file at the specified path into the output file.  Firmion treats all
input files as binary files.  Paths can be relative to the current directory or
absolute.

For example, given the file test_source_1.txt containing:

    Hello!

The following program simply copies these 6 UTF-8 characters to the output file.

    section foo {
        wrf "test_source_1.txt"; // Hello!
        assert sizeof(foo) == 6;
    }

    output foo;

---

## `wrs <expression> [, <expression>, ...];`

Evaluates the comma separated list of expressions and writes the resulting
string to the output file.  Wrs accepts the same expressions and operates
similarly to the print statement.  For more information, see
[print](#print).

The wrs statement does not implicitly write a terminating 0 byte after the
string.  Users creating null terminated (C style) strings in an output file
should add an explicit \0.

    wrs "my null terminated string\0";

---

# Built-in Variables

Firmion pre-defines built-in identifiers that begin with `__` (double underscore).
They can appear in any expression context that accepts the corresponding type.
As shown in the table below, some builtins cannot be used in `const` expressions
because their values depend on dynamic layout values.

| Variable                 | Type     | OK in `const`? | Description                                                                         |
| ------------------------ | -------- | -------------- | ----------------------------------------------------------------------------------- |
| `__OUTPUT_SIZE`          | `U64`    | No             | Total output size in bytes.  Equivalent to `sizeof(<output-section>)`.              |
| `__OUTPUT_ADDR`          | `U64`    | No             | Address of output section at SectionStart.  Equivalent to `addr(<output-section>)`. |
| `__FIRMION_VERSION_STRING` | `String` | Yes            | Firmion version as a string, e.g. `"4.3.2"`.                                          |
| `__FIRMION_VERSION_MAJOR`  | `U64`    | Yes            | Major version component, e.g. "4" in "4.3.2"                                        |
| `__FIRMION_VERSION_MINOR`  | `U64`    | Yes            | Minor version component, e.g. "3" in "4.3.2"                                        |
| `__FIRMION_VERSION_PATCH`  | `U64`    | Yes            | Patch version component, e.g. "2" in "4.3.2"                                        |

## `__OUTPUT_SIZE`

Returns the total size of the output file in bytes.

Example — write a 4-byte header field containing the total output size:

    section payload {
        wrs "Hello";
    }

    section hdr {
        wr32 __OUTPUT_SIZE;  // filled with total image size at link time
    }

    section image {
        wr hdr;
        wr payload;
        assert __OUTPUT_SIZE == sizeof(image);  // equivalent forms
    }

    output image;

## `__OUTPUT_ADDR`

Returns the absolute starting address of the output section.  Equivalent to
`addr(<output-section>)`.  **Without placing the [section](#section) in region,
`__OUTPUT_ADDR` is zero regardless of `set_addr` command internal to the output
section**.  This occurs because `set_addr` is a scoped operation.  A `set_addr`
within a [section](#section) affects address calculations for subsequent writes internal to
the section, not the logical start of the [section](#section) itself.

If user places the output [section](#section) in a [`region`](#region), then `__OUTPUT_ADDR`
is the starting address of the region.

Example — embed the output base address in a table:

    region TOP { addr = 0x0800_0000; size = 64K; }

    section vtable {
        wr32 __OUTPUT_ADDR;  // base address of the output image
    }

    section code {
        wrs "code";
    }

    section image in TOP {
        wr vtable;
        wr code;
        assert __OUTPUT_ADDR == addr(image);  // equivalent expressions
    }

    output image;

## `__FIRMION_VERSION_STRING`

Returns the Firmion tool version as a string (e.g. `"4.0.0"`).  The value is fixed
at compile time and may be used in `const` expressions, `wrs`, and `print`.

Example — stamp the tool version into a firmware header:

    section hdr {
        wrs __FIRMION_VERSION_STRING;
    }

    section image {
        wr hdr;
        wrs "payload";
    }

    output image;

## `__FIRMION_VERSION_MAJOR`, `__FIRMION_VERSION_MINOR`, `__FIRMION_VERSION_PATCH`

Return the individual numeric components of the Firmion version as `U64` values.
All three are fixed at compile time and may be used in `const` expressions and
arithmetic.

Example — pack the version into a 3-byte field and assert the tool is new enough:

    const MIN_MAJOR = 4u;

    section hdr {
        assert __FIRMION_VERSION_MAJOR >= MIN_MAJOR;
        wr8 __FIRMION_VERSION_MAJOR;
        wr8 __FIRMION_VERSION_MINOR;
        wr8 __FIRMION_VERSION_PATCH;
    }

    section image {
        wr hdr;
        wrs "payload";
    }

    output image;

---

# Firmion Extensions

Firmion supports compile time extensions to simplify the addition of new
functionality. This extension capability enables user defined hashing,
compression, validation and other binary data processing tasks.  The following
sections describe how extensions work and how to create them.

The command line option `--list-extensions` outputs the names of all available
extensions as enabled by Cargo feature flags.  The following table shows the
currently support standard extensions.

| Extension         | Description                                     |
| ----------------- | ----------------------------------------------- |
| std::crc32c       | 32-bit CRC32-C Castagnoli polynomial 0x1EDC6F41 |
| std::md5          | 128-bit MD5 hash                                |
| std::sha256       | 256-bit SHA-256 hash                            |
| std::esp_checksum | 8-bit simple XOR hash                           |

---

## Extensions Are A Compile-Time Feature

Extensions build and link to Firmion at compile time as controlled by Cargo
feature flags.  Because Rust does not guarantee a stable ABI between versions,
Firmion requires compile time construction to eliminate ABI incompatibilities and
enable the use of safe Rust.  The following bullets provide an overview of how
extensions work:

- Extensions interact with Firmion through the `FirmionExtension` trait.

- Extensions can read directly from the output buffer for a specified section
  via zero-copy and safe-memory slices (`&[u8]`).

- In addition to output buffer access, extensions can have their own input
  parameters like a normal function call.

- Extensions are identified by a **name** in a **namespace**.  Firmion reserves
  the namespaces `std` and `firmion`.

- Extensions report their fixed length binary footprint by implementing the
  `.size()` trait method. Firmion calls each extension's `.size()` method
  **exactly once** during output layout calculations and caches the result.
  Firmion always passes a mutable output slice (`&mut [u8]`) of the reported size
  to the extension's `.generate()` method.

- Extensions register themselves at compile time in Firmion's internal extension
  registry.

- The `FirmionExtension` trait interface allows extensions to return logging and
  error diagnostics integrated with Firmion's own diagnostic output.  See []

---

## Invoking Extensions

Users invoke extensions using function-style syntax.  Users creating their own extension can take any number of parameters of any Firmion support type:

    turbo::boost("Big", 1, -42, 0x12345678);

Fixed-size write commands like `wr32` are invalid for extensions. If the
designer needs to pad the extension's output to a specific size, they must
follow the `wr` command with a `pad_sec_offset` or `align` statement.

### Passing Section Data to Extensions

Users can pass the data in a [section](#section) to an extension by passing the
section name as a parameter.  Extensions take section data as an immutable
zero-copy slice parameter of Rust type &[u8].  Section data passed to the
extension at the time of the call includes all data generated by non-extension
write commands. Furthermore, the data includes the output of extensions executed
*before* the current extension.

As an example, consider the `std::crc32c` extension.  This extension generates a
CRC hash over the data provided by the specified section.  The extension
produces a 4-byte output.

    section foo_binary {
        wrf "foo.bin"; // Write the file foo.bin in this section.
    }

    section bar {
        wr foo_binary;
        // Write the CRC hash of everything in foo_binary
        wr std::crc32c(foo_binary);
    }

Users can also pass the section containing the extension's own output area to
the extension.  The extension receives a slice of the *full size* of the
section.  The extension's output area is zero-filled.  For example:

    section foo_binary {
        wrf "foo.bin";
        // Warning, the CRC input data is the full length of the foo_binary
        // section and includes 4 trailing zero bytes in place of the
        // extension's output.
        wr std::crc32c(foo_binary);
    }

    section bar {
        wr foo_binary;
    }

Likewise, if the section contains the output of an extension executing *after*
the current extension, Firmion zero-fills the area reserved for the later
extension's output.

### Named Parameters

To help eliminate bugs, Firmion extensions support named parameters.  Extensions
define their parameter names when registered.  In the example below, we call the
extension `custom::my_extension` passing it the required parameters
`data_section` and `code_section`.  The compiler passes the values by name in
the order expected by the extension, regardless of the order given at the call
site.

    //
    // extension example
    //
    section my_data {
        wrf "cool_data.bin";
    };

    section my_code {
        wrf "cool_code.bin";
    };

    section stuff {
        wr my_data;
        wr my_code;
        // Use named arguments to avoid positional and semantic bugs!
        custom::my_extension(data_section=my_data, code_section=my_code);
    };

---

## Size of Extension Output

Users can query the size of an extension's output using the `sizeof` command.
For example:

    assert sizeof(std::crc32c) == 4;

---

## Creating and Registering a New Extension

Extensions register through the `extensions` crate (`extensions/src/lib.rs`).
`process.rs` calls `extensions::register_all` once at startup; adding an
extension requires no changes outside `extensions/`.

### Step 1 — Create the extension crate

Place new extensions under `std/` for proposed standard library extensions, or
under a workspace path matching your namespace for third-party extensions.
Implement the `FirmionExtension` trait from the `firmion_extension` crate.

    // my_extension/src/lib.rs
    use firmion_extension::FirmionExtension;
    use extension_registry::ExtensionRegistry;

    pub struct MyExtension;

    impl FirmionExtension for MyExtension {
        fn name(&self) -> &str { "my_ns::my_ext" }
        fn size(&self) -> usize { 4 }
        fn execute(&self, _args: &[u64], img: &[u8], out: &mut [u8]) -> Result<(), String> {
            // write 4 bytes into out
            Ok(())
        }
    }

    pub fn register(registry: &mut ExtensionRegistry) {
        registry.register_ranged(Box::new(MyExtension));
    }

### Step 2 — Add the crate to the workspace

In the root `Cargo.toml`, add the crate path to `[workspace] members`.

### Step 3 — Wire into `extensions/`

In `extensions/Cargo.toml`, add the new crate as a dependency:

    my_extension = { path = "../my_extension" }

In `extensions/src/lib.rs`, call its register function inside `register_all`:

    pub fn register_all(registry: &mut ExtensionRegistry) {
        std_crc32c::register(registry);
        my_extension::register(registry);  // add this line
    }

### Step 4 — Add tests

Create a `tests/` directory in your extension crate with `.firm` scripts
and an `integration.rs` test file.  Use `CARGO_MANIFEST_DIR` to locate
`.firm` files relative to the workspace root — see
`std/crc32c/tests/integration.rs` for a complete example.

Run the extension's tests with:

    cargo test -p my_extension

---

# Firmion Development

This section provides notes for developers interested in contributing to Firmion.

## Unit Testing

Firmion relies on 100's of unit tests to catch bugs.  You can run these with:

    cargo test --all

## Fuzz Testing

Firmion supports fuzz tests for several of its internal libraries.  Fuzz testing
starts from a corpus of random inputs and then further randomizes those inputs
to try to cause crashes and hangs.  At the time of writing, fuzz testing
**requires the nightly build**.  See `fuzz_help.md` in the source repo for more
information.

## Checking Test Code Coverage

If you're using Windows as a development platform, then this worked for me to
install the llvm-cov tool.  I have the free version of Microsoft Visual Studio
installed.

    rustup component add llvm-tools
    cargo install cargo-llvm-cov --locked

To generate an ASCII table of coverage stats to the terminal:

    cargo llvm-cov --all-features --workspace

To update the coverage table in this README from Windows, run
`.\update_coverage.ps1`.

<!-- COVERAGE_START -->
```text
Filename                                            Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
ast/ast.rs                                             2452               519    78.83%          69                10    85.51%        1360               237    82.57%           0                 0         -
ast/lexer.rs                                            439                13    97.04%          16                 0   100.00%         268                10    96.27%           0                 0         -
astdb/astdb.rs                                          761               115    84.89%          12                 0   100.00%         374                37    90.11%           0                 0         -
const_eval/const_eval.rs                               1079               162    84.99%          40                 5    87.50%         640               124    80.62%           0                 0         -
depth_guard/depth_guard.rs                              146                 0   100.00%          17                 0   100.00%          77                 0   100.00%           0                 0         -
diags/diags.rs                                          282                28    90.07%          14                 1    92.86%         150                21    86.00%           0                 0         -
exec_phase/exec_phase.rs                                635               105    83.46%          16                 2    87.50%         423                51    87.94%           0                 0         -
extension_registry/extension_registry.rs                258                 9    96.51%          18                 3    83.33%         126                 9    92.86%           0                 0         -
extension_registry/test_mocks.rs                        274                34    87.59%          41                 6    85.37%         215                33    84.65%           0                 0         -
extensions/src/lib.rs                                    12                 0   100.00%           1                 0   100.00%           7                 0   100.00%           0                 0         -
extensions/std/crc32c/src/crc32c.rs                      31                 2    93.55%           5                 0   100.00%          26                 3    88.46%           0                 0         -
extensions/std/esp_checksum/src/esp_checksum.rs          66                 9    86.36%           6                 1    83.33%          66                16    75.76%           0                 0         -
extensions/std/md5/src/md5.rs                            31                 2    93.55%           5                 0   100.00%          26                 3    88.46%           0                 0         -
extensions/std/sha256/src/sha256.rs                      31                 2    93.55%           5                 0   100.00%          26                 3    88.46%           0                 0         -
extensions/std/xor/src/xor.rs                            31                 2    93.55%           6                 0   100.00%          26                 3    88.46%           0                 0         -
firmion_extension/lib.rs                                  3                 0   100.00%           1                 0   100.00%           3                 0   100.00%           0                 0         -
ir/ir.rs                                                316                31    90.19%          31                 1    96.77%         236                22    90.68%           0                 0         -
irdb/irdb.rs                                            794               100    87.41%          20                 2    90.00%         464                72    84.48%           0                 0         -
ireval/ireval.rs                                         85                 0   100.00%           4                 0   100.00%          58                 0   100.00%           0                 0         -
layout_phase/layout_phase.rs                           1701               359    78.89%          48                 2    95.83%        1088               201    81.53%           0                 0         -
layoutdb/layoutdb.rs                                    826               169    79.54%          19                 0   100.00%         501                78    84.43%           0                 0         -
linearizer/linearizer.rs                                838                64    92.36%          23                 1    95.65%         506                51    89.92%           0                 0         -
locationdb/locationdb.rs                                 39                 4    89.74%           3                 1    66.67%          28                 4    85.71%           0                 0         -
map_phase/map_phase.rs                                  893                14    98.43%          57                 0   100.00%         613                 9    98.53%           0                 0         -
objfile/objfile.rs                                      195                16    91.79%           5                 0   100.00%         116                 7    93.97%           0                 0         -
output_buffer/output_buffer.rs                           55                 8    85.45%          10                 2    80.00%          33                 6    81.82%           0                 0         -
process/process.rs                                      446                24    94.62%          28                 5    82.14%         252                 9    96.43%           0                 0         -
regiondb/regiondb.rs                                    127                 5    96.06%           3                 0   100.00%         107                 5    95.33%           0                 0         -
src/main.rs                                             164                14    91.46%          11                 3    72.73%         108                10    90.74%           0                 0         -
symtable/symtable.rs                                    107                 5    95.33%          14                 2    85.71%          78                 5    93.59%           0                 0         -
validation_phase/validation_phase.rs                     39                 2    94.87%           1                 0   100.00%          24                 0   100.00%           0                 0         -
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
TOTAL                                                 13156              1817    86.19%         549                47    91.44%        8025              1029    87.18%           0                 0         -
```
<!-- COVERAGE_END -->

## Firmion Source Code Overview

| File                                     | Stage         | Summary                                                                                 |
| ---------------------------------------- | ------------- | --------------------------------------------------------------------------------------- |
| ast/ast.rs                               | Stage 1       | Hand-rolled lexer -> token stream -> arena AST -> AstDb validation                      |
| const_eval/const_eval.rs                 | Stage 2       | Lowers const and region AST statements to LinIR, returns SymbolTable and RegionBindings |
| prune/prune.rs                           | Stage 3       | Eliminates if/else nodes from the AST; promotes sections from the taken branch          |
| layoutdb/layoutdb.rs                     | Stage 4       | AST flattening into linear IR and operand vectors; values are still strings             |
| irdb/irdb.rs                             | Stage 5       | String to typed value conversion, operand and file validation                           |
| layout_phase/layout_phase.rs             | Stage 6       | Iterative address resolution and section footprint calculation                          |
| validation_phase/validation_phase.rs     | Stage 7       | Evaluates all assert instructions after layout and before binary output                 |
| exec_phase/exec_phase.rs                 | Stage 8       | Writes inline data, padding, file contents, and extension output to binary              |
| symtable/symtable.rs                     | Shared types  | SymbolTable tracking every compile-time const from declaration through use              |
| linearizer/linearizer.rs                 | Shared types  | LinIR and LinOperand types; shared lowering infrastructure for stages 2 and 4           |
| ir/ir.rs                                 | Shared types  | IRKind, ParameterValue, IROperand, IR — the data flowing between stages 4–8             |
| locationdb/locationdb.rs                 | Shared types  | LocationDb and Location produced by stage 6 and consumed by stages 7 and 8              |
| map_phase/map_phase.rs                   | Map output    | Builds MapDb from LocationDb and IRDb; renders map to CSV, JSON, C99, and RS            |
| process/process.rs                       | Orchestrator  | Orchestration of all stages, parses `-D` defines, opens the output file                 |
| diags/diags.rs                           | Cross-cutting | Ariadne-backed diagnostic output channel used by every stage                            |
| extensions/src/lib.rs                    | Extensions    | Single registration point for all extensions                                            |
| firmion_extension/lib.rs                   | Extensions    | Public API for extension authors                                                        |
| extension_registry/extension_registry.rs | Extensions    | Runtime extension registry and dispatch wrapper                                         |
| std/crc32c/src/lib.rs                    | std extension | CRC-32C (Castagnoli) hash over caller-specified output region                           |
| std/sha256/src/lib.rs                    | std extension | SHA256 hash over caller-specified output region                                         |

## Rebuilding the vscode Syntax Highlighting Extension

Rebuilding the extension require Node.js.  After you install Node.js, you may
need to restart your command prompt.

Building the extension requires
[vsce](https://github.com/microsoft/vscode-vsce).  One time, you'll need to use
`npm` to install `vsce`

    npm install -g @vscode/vsce

Now you're ready to rebuild the extension.

    cd vscode-firmion
    vsce package

To install the extension into vscode locally:

    code --install-extension vscode-firmion-0.1.0.vsix






