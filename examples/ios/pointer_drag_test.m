#import <UIKit/UIKit.h>
#import <XCTest/XCTest.h>

static XCUIElement *FindDivider(XCUIApplication *app, BOOL vertical) {
  XCUIElementQuery *all = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElementQuery *dividers = [all matchingPredicate:[NSPredicate predicateWithFormat:@"label == %@", @"Pane divider 1"]];
  for (XCUIElement *divider in dividers.allElementsBoundByIndex) {
    CGRect frame = divider.frame;
    if (vertical ? CGRectGetHeight(frame) > CGRectGetWidth(frame)
                 : CGRectGetWidth(frame) > CGRectGetHeight(frame)) {
      return divider;
    }
  }
  return nil;
}

static void DragPointer(XCUICoordinate *start, CGVector offset) {
  XCUICoordinate *end = [start coordinateWithOffset:offset];
  [start clickForDuration:0.10 thenDragToCoordinate:end withVelocity:XCUIGestureVelocitySlow
        thenHoldForDuration:0.05];
}

static BOOL ShowsTooltip(XCUIApplication *app, CGRect owner) {
  UIImage *screen = XCUIScreen.mainScreen.screenshot.image;
  CGImageRef screenshot = screen.CGImage;
  if (screenshot == NULL) {
    return NO;
  }
  CGFloat scale = CGImageGetWidth(screenshot) / app.frame.size.width;
  CGFloat bottom = CGRectGetMaxY(owner);
  CGRect region = CGRectMake(0, bottom * scale, CGImageGetWidth(screenshot), 58 * scale);
  CGImageRef crop = CGImageCreateWithImageInRect(screen.CGImage, CGRectIntegral(region));
  if (crop == NULL) {
    return NO;
  }
  size_t width = CGImageGetWidth(crop), height = CGImageGetHeight(crop);
  NSMutableData *image = [NSMutableData dataWithLength:width * height * 4];
  CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  CGContextRef context = CGBitmapContextCreate(image.mutableBytes, width, height, 8, width * 4,
      space, (CGBitmapInfo)kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
  NSUInteger green = 0;
  if (context != NULL) {
    CGContextDrawImage(context, CGRectMake(0, 0, width, height), crop);
    const uint8_t *rgba = image.bytes;
    for (size_t i = 0; i < width * height * 4; i += 4) {
      if (rgba[i] < 80 && rgba[i + 1] > 190 && rgba[i + 2] < 155) {
        green++;
      }
    }
    CGContextRelease(context);
  }
  CGColorSpaceRelease(space);
  CGImageRelease(crop);
  return green > 300;
}

@interface XRackPointerDragTest : XCTestCase
@end

@implementation XRackPointerDragTest

- (void)testTouchTooltipPaintsWithoutActivatingAndAllowsTapAndScroll {
  self.continueAfterFailure = NO;
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--touch-tooltip"];
  [app launch];

  XCUIElement *inspect = app.buttons[@"Inspect sensor"];
  XCTAssertTrue([inspect waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  CGRect owner = inspect.frame;
  XCTAssertFalse(ShowsTooltip(app, owner));
  [inspect pressForDuration:1.0];
  XCTAssertTrue(ShowsTooltip(app, owner));
  XCTAssertTrue(app.staticTexts[@"Inspections 0"].exists);
  [inspect tap];
  XCTAssertTrue([app.staticTexts[@"Inspections 1"] waitForExistenceWithTimeout:5]);
  XCTAssertFalse(ShowsTooltip(app, owner));

  XCUICoordinate *start = [inspect coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.75)];
  XCUICoordinate *end = [start coordinateWithOffset:CGVectorMake(0, -72)];
  [start pressForDuration:0.06 thenDragToCoordinate:end withVelocity:XCUIGestureVelocityFast
           thenHoldForDuration:0];
  XCTAssertTrue(!inspect.exists || CGRectGetMinY(inspect.frame) < CGRectGetMinY(owner) - 20);
  XCTAssertTrue(app.staticTexts[@"Inspections 1"].exists);
  XCTAssertFalse(ShowsTooltip(app, owner));
}

- (void)testStationaryHoldOnSelectedTextOpensEditMenu {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--text-drag"];
  [app launch];

  XCUIElement *select = app.buttons[@"Select sample"];
  XCTAssertTrue([select waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [select tap];
  XCUIElement *sample = [app descendantsMatchingType:XCUIElementTypeAny][@"Drag sample"];
  XCTAssertEqualObjects(sample.value, @"A🧪B");
  XCUICoordinate *highlight = [[sample coordinateWithNormalizedOffset:CGVectorMake(0, 0.5)]
      coordinateWithOffset:CGVectorMake(25, 0)];
  [highlight pressForDuration:1.0];
  XCUIElement *copy = app.menuItems[@"Copy"];
  XCTAssertTrue([copy waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertEqualObjects(sample.value, @"A🧪B");
}

- (void)testIPadPrimaryPointerSelectsTextAndResizesDividers {
  XCTSkipUnless(XCUIDevice.sharedDevice.supportsPointerInteraction,
                @"Requires a simulator with pointing-device input");
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--pointer-styles"];
  [app launch];

  XCUIElement *name = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection name"];
  XCTAssertTrue([name waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCUIElement *source = [app descendantsMatchingType:XCUIElementTypeAny][@"Test input source"];
  XCTAssertTrue([source waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCUICoordinate *probe = [source coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  [probe click];
  XCTAssertTrue([app.staticTexts[@"Input Mouse"] waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [probe tap];
  XCTAssertTrue([app.staticTexts[@"Input Touch"] waitForExistenceWithTimeout:5], @"%@", app.debugDescription);

  for (NSNumber *vertical in @[@YES, @NO]) {
    XCUIElement *divider = FindDivider(app, vertical.boolValue);
    XCTAssertNotNil(divider, @"%@", app.debugDescription);
    CGRect original = divider.frame;
    DragPointer([divider coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)],
                vertical.boolValue ? CGVectorMake(54, 0) : CGVectorMake(0, 24));
    NSPredicate *moved = [NSPredicate predicateWithBlock:^BOOL(XCUIElement *element, NSDictionary *bindings) {
      CGRect frame = element.frame;
      return vertical.boolValue ? CGRectGetMidX(frame) >= CGRectGetMidX(original) + 18
                                : CGRectGetMidY(frame) >= CGRectGetMidY(original) + 10;
    }];
    [self expectationForPredicate:moved evaluatedWithObject:divider handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
  }

  XCTAssertTrue(app.staticTexts[@"No text selected"].exists);
  XCUICoordinate *left = [[name coordinateWithNormalizedOffset:CGVectorMake(0, 0.5)]
      coordinateWithOffset:CGVectorMake(5, 0)];
  DragPointer(left, CGVectorMake(240, 0));
  XCTAssertTrue([app.staticTexts[@"Selected Pump inspection"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertEqualObjects(name.value, @"Pump inspection");
}

- (void)testIPadDoubleClickSelectsTheWordUnderThePointer {
  XCTSkipUnless(XCUIDevice.sharedDevice.supportsPointerInteraction,
                @"Requires a simulator with pointing-device input");
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--pointer-styles"];
  [app launch];

  XCUIElement *name = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection name"];
  XCTAssertTrue([name waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCUICoordinate *pump = [[name coordinateWithNormalizedOffset:CGVectorMake(0, 0.5)]
      coordinateWithOffset:CGVectorMake(24, 0)];
  [pump doubleClick];
  XCTAssertTrue([app.staticTexts[@"Selected Pump"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertEqualObjects(name.value, @"Pump inspection");
  [app typeKey:@"7" modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"7 inspection"]
           evaluatedWithObject:name handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(app.staticTexts[@"No text selected"].exists);
}

@end
