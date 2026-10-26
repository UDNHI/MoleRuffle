// Reproducible MoleRuffle monogram; no game artwork or system fonts required.
#import <Cocoa/Cocoa.h>
static NSColor *rgb(CGFloat r, CGFloat g, CGFloat b) {
    return [NSColor colorWithSRGBRed:r/255 green:g/255 blue:b/255 alpha:1];
}
int main(int argc, char **argv) { @autoreleasepool {
    if (argc != 2) return 1;
    NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc]
        initWithBitmapDataPlanes:NULL pixelsWide:1024 pixelsHigh:1024
        bitsPerSample:8 samplesPerPixel:4 hasAlpha:YES isPlanar:NO
        colorSpaceName:NSCalibratedRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
    [NSGraphicsContext saveGraphicsState];
    NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:bitmap];
    NSBezierPath *tile = [NSBezierPath bezierPathWithRoundedRect:NSMakeRect(48,48,928,928)
        xRadius:206 yRadius:206];
    NSGradient *background = [[NSGradient alloc]
        initWithStartingColor:rgb(18,61,57) endingColor:rgb(39,101,86)];
    [background drawInBezierPath:tile angle:65];
    // Broad, round strokes stay legible at Dock and Finder sizes.
    NSBezierPath *letter = [NSBezierPath bezierPath];
    [letter moveToPoint:NSMakePoint(280,324)];
    [letter lineToPoint:NSMakePoint(280,694)];
    [letter lineToPoint:NSMakePoint(512,474)];
    [letter lineToPoint:NSMakePoint(744,694)];
    [letter lineToPoint:NSMakePoint(744,324)];
    letter.lineWidth=100;
    letter.lineCapStyle=NSLineCapStyleRound;
    letter.lineJoinStyle=NSLineJoinStyleRound;
    [rgb(255,246,222) setStroke]; [letter stroke];
    // One warm accent, without using the official character or logo.
    [rgb(235,115,79) setFill];
    [[NSBezierPath bezierPathWithOvalInRect:NSMakeRect(460,245,104,104)] fill];
    [NSGraphicsContext restoreGraphicsState];
    NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    return [png writeToFile:[NSString stringWithUTF8String:argv[1]] atomically:YES] ? 0 : 1;
} }
