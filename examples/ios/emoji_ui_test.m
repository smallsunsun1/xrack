#import <XCTest/XCTest.h>
#import <UIKit/UIKit.h>

static NSUInteger ChromaticPixels(UIImage *screenshot, CGRect frame, CGRect screen) {
  CGImageRef image = screenshot.CGImage;
  CGFloat scale = CGImageGetWidth(image) / screen.size.width;
  CGRect pixels = CGRectMake((frame.origin.x - screen.origin.x) * scale,
                             (frame.origin.y - screen.origin.y) * scale,
                             frame.size.width * scale, frame.size.height * scale);
  CGImageRef crop = CGImageCreateWithImageInRect(image, CGRectIntegral(pixels));
  if (crop == NULL) {
    return 0;
  }
  size_t width = CGImageGetWidth(crop), height = CGImageGetHeight(crop);
  NSMutableData *data = [NSMutableData dataWithLength:width * height * 4];
  CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  CGContextRef context = CGBitmapContextCreate(data.mutableBytes, width, height, 8, width * 4,
      space, (CGBitmapInfo)kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
  NSUInteger count = 0;
  if (context != NULL) {
    CGContextDrawImage(context, CGRectMake(0, 0, width, height), crop);
    const uint8_t *rgba = data.bytes;
    for (size_t i = 0; i < width * height * 4; i += 4) {
      uint8_t low = MIN(rgba[i], MIN(rgba[i + 1], rgba[i + 2]));
      uint8_t high = MAX(rgba[i], MAX(rgba[i + 1], rgba[i + 2]));
      if (high - low >= 60 && high >= 110) {
        count++;
      }
    }
    CGContextRelease(context);
  }
  CGColorSpaceRelease(space);
  CGImageRelease(crop);
  return count;
}

@interface XRackEmojiUITest : XCTestCase
@end

@implementation XRackEmojiUITest

- (void)testEditorAndEmojiSequencesPaintColorOnTheMetalSurface {
  self.continueAfterFailure = NO;
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--emoji"];
  [app launch];
  XCUIElementQuery *allElements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *editor = allElements[@"Emoji editor"];
  XCTAssertTrue([editor waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCTAssertEqualObjects(editor.value, @"A🧪B");
  NSArray<NSString *> *samples = @[@"🧭", @"👩🏽‍💻", @"🇨🇳"];
  NSMutableArray<XCUIElement *> *elements = [NSMutableArray arrayWithObject:editor];
  for (NSString *sample in samples) {
    XCUIElement *element = allElements[sample];
    XCTAssertTrue(element.exists, @"Missing %@", sample);
    [elements addObject:element];
  }
  // The three labels share the same layout and size. A decomposed flag or
  // ZWJ sequence takes more than one glyph's advance.
  CGFloat glyphWidth = allElements[samples.firstObject].frame.size.width;
  for (NSString *sample in [samples subarrayWithRange:NSMakeRange(1, samples.count - 1)]) {
    XCTAssertEqualWithAccuracy(allElements[sample].frame.size.width, glyphWidth, 4.0,
                               @"Expected one shaped advance for %@", sample);
  }
  // The dark backing and white labels stay below this chroma threshold.
  // Each field in the assertion has to contain real color from the glyph.
  XCUIScreenshot *screenshot = XCUIScreen.mainScreen.screenshot;
  XCTAttachment *attachment = [XCTAttachment attachmentWithScreenshot:screenshot];
  attachment.name = @"XRack emoji";
  attachment.lifetime = XCTAttachmentLifetimeKeepAlways;
  [self addAttachment:attachment];
  for (XCUIElement *element in elements) {
    XCTAssertGreaterThan(ChromaticPixels(screenshot.image, element.frame, app.frame),
                         20u, @"Expected color pixels for %@", element.label);
  }
}

@end
