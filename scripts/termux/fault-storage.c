// Test-only ENOSPC injection for writes under one isolated temporary directory.
// Never used by the server or installation scripts; no filesystem is filled.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

ssize_t write(int fd, const void *buffer, size_t count) {
    const char *root = getenv("SONICBOOM_TEST_FULL_STORAGE");
    if (root && *root && fd >= 3) {
        char descriptor[64], path[4096];
        snprintf(descriptor, sizeof(descriptor), "/proc/self/fd/%d", fd);
        ssize_t length = readlink(descriptor, path, sizeof(path) - 1);
        if (length >= 0) {
            path[length] = '\0';
            if (strncmp(path, root, strlen(root)) == 0) {
                errno = ENOSPC;
                return -1;
            }
        }
    }
    typedef ssize_t (*write_fn)(int, const void *, size_t);
    write_fn next = (write_fn)dlsym(RTLD_NEXT, "write");
    if (!next) { errno = EIO; return -1; }
    return next(fd, buffer, count);
}
