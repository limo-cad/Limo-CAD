/* Regression for CVE-2026-85091. Build with AddressSanitizer on a POSIX host. */
#include "zlib.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    int fds[2], error;
    unsigned char fill[4096] = {0};
    unsigned char *input = malloc(1024 * 1024);
    uint32_t random = 1;
    assert(argc == 2 && input != NULL);
    for (size_t i = 0; i < 1024 * 1024; ++i) {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        input[i] = (unsigned char)random;
    }
    assert(pipe(fds) == 0);
    assert(fcntl(fds[1], F_SETFL, O_NONBLOCK) == 0);
    while (write(fds[1], fill, sizeof(fill)) > 0) {}
    assert(errno == EAGAIN || errno == EWOULDBLOCK);

    gzFile stream = gzdopen(fds[1], argv[1]);
    assert(stream != NULL && gzbuffer(stream, 64) == 0);
    int consumed = gzwrite(stream, input, 1024 * 1024);
    assert(consumed >= 0 && consumed < 1024 * 1024);
    gzerror(stream, &error);
    assert(error == Z_ERRNO);

    /* A retry with a small buffer must not write through the previous caller's
       buffer. The unpatched source overflows here in both wb and wbT modes. */
    gzclearerr(stream);
    assert(gzwrite(stream, "tail", 4) == 4);
    /* This is still a full non-blocking pipe, so final flushing must fail. */
    assert(gzclose(stream) == Z_ERRNO);
    close(fds[0]);
    free(input);
    puts("non-blocking write retry passed");
    return 0;
}
