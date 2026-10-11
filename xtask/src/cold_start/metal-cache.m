#import <Foundation/Foundation.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef void (*SetCachePath)(NSString *);
typedef NSString *(*GetCachePath)(void);

static void fail(const char *message) {
    fprintf(stderr, "Limo cold-launch check failed: %s\n", message);
    _exit(78);
}

static void privateDirectory(const char *path) {
    struct stat info;
    if (lstat(path, &info) || !S_ISDIR(info.st_mode) ||
        info.st_uid != getuid() || (info.st_mode & 0077)) {
        fail("cache folder must be a real, private directory owned by this user");
    }
}

// Test-only instrumentation. The release executable and shaders are unchanged.
// No default cache getter or GPU device is used before the override is installed.
__attribute__((constructor)) static void isolateShaderCache(void) {
    @autoreleasepool {
        const char *base = getenv("LIMO_CAD_COLD_CACHE_ROOT");
        if (!base || base[0] != '/') fail("missing absolute cold-cache root");
        privateDirectory(base);
        NSString *basePath = [NSString stringWithUTF8String:base];
        const char *inherited = getenv("LIMO_CAD_COLD_CACHE_ACTIVE");
        const char *parent = getenv("LIMO_CAD_COLD_CACHE_PARENT_PID");
        BOOL sharedWithChild = inherited && parent && strtol(parent, NULL, 10) == getppid();
        NSString *cachePath = nil;
        if (sharedWithChild) {
            cachePath = [NSString stringWithUTF8String:inherited];
            if (![[cachePath stringByDeletingLastPathComponent] isEqualToString:basePath]) {
                fail("child cache is outside the dedicated test root");
            }
            privateDirectory(inherited);
        } else {
            NSString *pattern = [basePath stringByAppendingPathComponent:@"launch.XXXXXX"];
            char *fresh = strdup(pattern.fileSystemRepresentation);
            if (!fresh || !mkdtemp(fresh)) fail("could not create a fresh cache");
            cachePath = [NSString stringWithUTF8String:fresh];
            if (setenv("LIMO_CAD_COLD_CACHE_ACTIVE", fresh, 1)) fail("could not share the launch cache");
            free(fresh);
        }
        char currentPid[32];
        snprintf(currentPid, sizeof(currentPid), "%d", getpid());
        if (setenv("LIMO_CAD_COLD_CACHE_PARENT_PID", currentPid, 1)) fail("could not identify the launch parent");
        void *metal = dlopen("/System/Library/Frameworks/Metal.framework/Metal", RTLD_NOW | RTLD_LOCAL);
        if (!metal) fail("Metal could not be loaded");
        SetCachePath setPath = (SetCachePath)dlsym(metal, "MTLSetShaderCachePath");
        if (!setPath) fail("this macOS does not support the test cache override");
        setPath(cachePath);
        NSMutableDictionary *paths = [NSMutableDictionary dictionary];
        for (NSString *symbol in @[@"MTLGetShaderCachePath", @"MTLGetModulesCachePath", @"MTLGetGPUArchiverCachePath"]) {
            GetCachePath getPath = (GetCachePath)dlsym(metal, symbol.UTF8String);
            if (!getPath) fail("Metal cache path cannot be verified");
            NSString *path = getPath();
            if (![path isKindOfClass:[NSString class]] ||
                (![path isEqualToString:cachePath] && ![path hasPrefix:[cachePath stringByAppendingString:@"/"]])) {
                fail("Metal would use an existing cache instead of the isolated cache");
            }
            paths[symbol] = path;
        }
        NSArray *existing = [[NSFileManager defaultManager] contentsOfDirectoryAtPath:cachePath error:nil];
        if (!existing) fail("new cache contents cannot be checked");
        if (!sharedWithChild && existing.count) fail("the new launch cache is not empty");
        NSDictionary *report = @{
            @"process_id": @(getpid()),
            @"cache_root": cachePath,
            @"inherited_from_same_launch": @(sharedWithChild),
            @"empty_before_gpu_initialization": (!sharedWithChild && existing.count == 0) ? @YES : @NO,
            @"verified_metal_paths": paths,
            @"test_only_private_metal_api": @YES,
            @"recorded_utc": [[NSDate date] description]
        };
        NSError *error = nil;
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted error:&error];
        NSString *name = [NSString stringWithFormat:@"cache-setup-%d.json", getpid()];
        if (!json || ![json writeToFile:[cachePath stringByAppendingPathComponent:name] options:NSDataWritingAtomic error:&error]) {
            fail("the cache-isolation receipt could not be saved");
        }
    }
}
