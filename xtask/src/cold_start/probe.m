#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef NSString *(*GetCache)(void);

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        (void)argc; (void)argv;
        const char *selected = getenv("LIMO_CAD_COLD_CACHE_ACTIVE");
        if (!selected) { fprintf(stderr,"Cold-cache constructor did not run\n"); return 65; }
        NSString *root = [NSString stringWithUTF8String:selected];
        void *handle = dlopen("/System/Library/Frameworks/Metal.framework/Metal", RTLD_NOW);
        NSMutableDictionary *report = [@{@"cache_root":root,@"process_id":@(getpid())} mutableCopy];
        for (NSString *name in @[@"MTLGetShaderCachePath", @"MTLGetModulesCachePath", @"MTLGetGPUArchiverCachePath"]) {
            GetCache getter = (GetCache)dlsym(handle, name.UTF8String);
            if (!getter) return 66;
            NSString *path = getter();
            if (![path isKindOfClass:[NSString class]] || !path.isAbsolutePath) { fprintf(stderr, "Unexpected path format: %s\n",name.UTF8String); return 67; }
            if (![path isEqualToString:root] && ![path hasPrefix:[root stringByAppendingString:@"/"]]) { fprintf(stderr,"Cache was not isolated: %s\n",path.UTF8String); return 68; }
            report[name] = path;
        }
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr,"No native GPU device\n"); return 69; }
        report[@"device"] = device.name;
        NSString *source = @"#include <metal_stdlib>\nusing namespace metal;\nkernel void limo_cold_probe(device uint* result [[buffer(0)]], uint id [[thread_position_in_grid]]) { result[id] = id + 7u; }\n";
        NSError *error = nil;
        NSTimeInterval started = NSProcessInfo.processInfo.systemUptime;
        id<MTLLibrary> library = [device newLibraryWithSource:source options:nil error:&error];
        if (!library) { fprintf(stderr,"Metal library failed: %s\n",error.localizedDescription.UTF8String); return 70; }
        id<MTLFunction> function = [library newFunctionWithName:@"limo_cold_probe"];
        id<MTLComputePipelineState> pipeline = [device newComputePipelineStateWithFunction:function error:&error];
        if (!pipeline) { fprintf(stderr,"Metal pipeline failed: %s\n",error.localizedDescription.UTF8String); return 71; }
        report[@"compile_seconds"] = @(NSProcessInfo.processInfo.systemUptime - started);
        id<MTLBuffer> output = [device newBufferWithLength:4*sizeof(uint32_t) options:MTLResourceStorageModeShared];
        id<MTLCommandQueue> queue = [device newCommandQueue];
        id<MTLCommandBuffer> commands = [queue commandBuffer];
        id<MTLComputeCommandEncoder> encoder = [commands computeCommandEncoder];
        [encoder setComputePipelineState:pipeline];
        [encoder setBuffer:output offset:0 atIndex:0];
        [encoder dispatchThreads:MTLSizeMake(4,1,1) threadsPerThreadgroup:MTLSizeMake(4,1,1)];
        [encoder endEncoding];
        [commands commit];
        [commands waitUntilCompleted];
        if (commands.status != MTLCommandBufferStatusCompleted) return 72;
        uint32_t *values = output.contents;
        if (values[0] != 7 || values[1] != 8 || values[2] != 9 || values[3] != 10) return 73;
        report[@"gpu_result"] = @[@(values[0]),@(values[1]),@(values[2]),@(values[3])];
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted error:&error];
        fwrite(json.bytes,1,json.length,stdout); fputc('\n',stdout);
        return 0;
    }
}
