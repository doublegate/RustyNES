/* SPDX-License-Identifier: CC0-1.0 */
/*
 * RustyNES T-DEBUG-DAP fixture (v3.6.0): the smallest C program that has a
 * global, a function with a parameter and a local, and a caller with a local,
 * so a source-level debugger has one of each to stop at, step through and
 * read. Built twice by build.py: with cc65 (`.dbg` debug info) and with the
 * llvm-mos SDK (DWARF in the ELF). Written for RustyNES; dedicated to the
 * public domain under CC0 1.0.
 *
 * The `BP:` markers are how the tests find line numbers, so editing this
 * file does not silently move a breakpoint onto the wrong line.
 */
unsigned char counter;

static unsigned char add3(unsigned char v)
{
    unsigned char r;
    r = v + 3;          /* BP:add3-body */
    return r;           /* BP:add3-return */
}

int main(void)
{
    unsigned char local = 5;
    for (;;) {
        local = add3(local);   /* BP:main-call */
        counter = local;       /* BP:main-store */
    }
}
