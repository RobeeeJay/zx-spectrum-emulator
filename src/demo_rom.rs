//! A tiny built-in ROM used when no real 48K ROM image is present.
//!
//! It renders an animated pattern into a back buffer at $8000 and then LDIRs
//! it into video RAM, which exercises the RAM heat map, the slow-draw mode and
//! the back-buffer detector without needing any copyrighted ROM.
//!
//! ```text
//!         DI
//!         LD   A,7
//!         OUT  (254),A
//!         LD   SP,$6000
//!         LD   C,0
//! main:   LD   HL,$8000
//!         LD   D,$18          ; 24 pages of bitmap
//! fill:   LD   A,L
//!         XOR  C
//!         LD   (HL),A
//!         INC  HL
//!         LD   A,L
//!         OR   A
//!         JR   NZ,fill
//!         DEC  D
//!         JR   NZ,fill
//!         LD   D,3            ; 3 pages of attributes
//! attr:   LD   A,C
//!         AND  7
//!         OR   $38
//!         LD   (HL),A
//!         INC  HL
//!         LD   A,L
//!         OR   A
//!         JR   NZ,attr
//!         DEC  D
//!         JR   NZ,attr
//!         LD   HL,$8000       ; flip the buffer to the screen
//!         LD   DE,$4000
//!         LD   BC,$1B00
//!         LDIR
//!         INC  C
//!         JP   main
//! ```
pub const DEMO_ROM: [u8; 0x39] = [
    0xf3, // DI
    0x3e, 0x07, // LD A,7
    0xd3, 0xfe, // OUT (254),A
    0x31, 0x00, 0x60, // LD SP,$6000
    0x0e, 0x00, // LD C,0
    0x21, 0x00, 0x80, // main: LD HL,$8000
    0x16, 0x18, // LD D,$18
    0x7d, // fill: LD A,L
    0xa9, // XOR C
    0x77, // LD (HL),A
    0x23, // INC HL
    0x7d, // LD A,L
    0xb7, // OR A
    0x20, 0xf8, // JR NZ,fill
    0x15, // DEC D
    0x20, 0xf5, // JR NZ,fill
    0x16, 0x03, // LD D,3
    0x79, // attr: LD A,C
    0xe6, 0x07, // AND 7
    0xf6, 0x38, // OR $38
    0x77, // LD (HL),A
    0x23, // INC HL
    0x7d, // LD A,L
    0xb7, // OR A
    0x20, 0xf5, // JR NZ,attr
    0x15, // DEC D
    0x20, 0xf2, // JR NZ,attr
    0x21, 0x00, 0x80, // LD HL,$8000
    0x11, 0x00, 0x40, // LD DE,$4000
    0x01, 0x00, 0x1b, // LD BC,$1B00
    0xed, 0xb0, // LDIR
    0x0c, // INC C
    0xc3, 0x0a, 0x00, // JP main
];
