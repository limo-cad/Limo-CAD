# zlib non-blocking write backport

This overlay selects **zlib 1.3.2#3** from Microsoft's vcpkg commit
[`520c3b781ac2abf391887d286d569f74ae6bf752`](https://github.com/microsoft/vcpkg/commit/520c3b781ac2abf391887d286d569f74ae6bf752).
The six upstream port files are retained, including their source archive SHA512
and the patch's original filename. The recipe is covered by
[`VCPKG-LICENSE.txt`](VCPKG-LICENSE.txt); the installed library retains its zlib license.

Despite the filename `CVE-2026-76844.patch`, its contents backport zlib commit
[`df84af25dc1942490e1d1c899a07619152a46148`](https://github.com/madler/zlib/commit/df84af25dc1942490e1d1c899a07619152a46148),
which fixes **CVE-2026-85091**. When a direct caller-buffer write stalls, the
repair drops the remaining borrowed input and resets the input pointer so a
subsequent small write uses zlib's own buffer. This preserves the 1.3.2 ABI and
`z.dll` name. The rest of the registry remains at the existing baseline.

The existing `native/occt-overlay` configuration applies to local builds and
both Windows architectures. Its files are already part of the installed SDK
and binary cache keys, so the vulnerable cached SDK cannot be an exact hit.

Run the regression on macOS or Linux with Python 3, a C compiler supporting
AddressSanitizer, `tar` and `patch`:

```sh
CC=clang python3 native/occt-overlay/zlib/verify.py
```

The script verifies the archive against the selected port's SHA512, applies
all patches from its recipe, exercises large-to-small write retries on a full
non-blocking pipe in compressed and direct modes, and runs zlib's upstream
example. It then removes only the repair and requires AddressSanitizer to detect
the invalid write in each mode. Use `--archive PATH` for an already downloaded
archive; checksum validation still applies. Repository CI runs the same check.

This is source regression evidence. The 2026-11-17 package review also verified
the Windows x64/ARM64 archives, their checksum-bound zlib 1.3.2#3 metadata and
all shipped DLL consumers. The native scanner still identifies this library as
1.3.2. The reviewed exception covers only final 0.3.0 Windows package paths;
every new build must pass the source regression, DLL consumer guard, inventory
verification and advisory scan. See the
[dependency review record](../../../docs/security/dependencies.md).
