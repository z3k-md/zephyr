Signed PawnIO modules, unchanged, from https://github.com/namazso/PawnIO.Modules release 0.2.11:

- `AMDFamily17.bin`: reads the CPU's temperature register on AMD Zen (families 17h to 1Ah).
- `IntelMSR.bin`: reads the package thermal status on Intel CPUs.

They are licensed under the GNU Lesser General Public License 2.1 (`COPYING`). Zephyr's CPU
temperature helper loads them into the PawnIO driver (https://pawnio.eu) when the user turns
the reading on in Settings › Resources.
