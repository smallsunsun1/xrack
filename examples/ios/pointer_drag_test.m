#import <UIKit/UIKit.h>
#import <XCTest/XCTest.h>
#import <objc/message.h>

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

static void SetSoftwareKeyboardAutomaticMinimizationInSimulator(BOOL enabled) {
#if TARGET_OS_SIMULATOR
  // XCTest can expose an off-screen keyboard tree when the simulator minimizes it.
  Class implementation = NSClassFromString(@"UIKeyboardImpl");
  SEL sharedInstance = NSSelectorFromString(@"sharedInstance");
  SEL setMinimization = NSSelectorFromString(@"setAutomaticMinimizationEnabled:");
  if ([implementation respondsToSelector:sharedInstance]) {
    id keyboard = ((id (*)(id, SEL))objc_msgSend)(implementation, sharedInstance);
    if ([keyboard respondsToSelector:setMinimization]) {
      ((void (*)(id, SEL, BOOL))objc_msgSend)(keyboard, setMinimization, enabled);
    }
  }
#endif
}

static BOOL SoftwareKeyboardVisible(XCUIApplication *app) {
  XCUIElement *keyboard = app.keyboards.firstMatch;
  XCUIElement *letter = [keyboard.keys matchingPredicate:
      [NSPredicate predicateWithFormat:@"label ==[c] %@", @"q"]].firstMatch;
  return keyboard.exists && CGRectIntersectsRect(app.frame, keyboard.frame) && letter.hittable;
}

@interface XRackPointerDragTest : XCTestCase
@end

@implementation XRackPointerDragTest

- (void)testLandscapeKeyboardKeepsTheScrollableDialogAndItsSaveActionUsable {
  self.continueAfterFailure = NO;
  SetSoftwareKeyboardAutomaticMinimizationInSimulator(NO);
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationPortrait;
  [self addTeardownBlock:^{ device.orientation = UIDeviceOrientationPortrait; }];
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--keyboard-layout", @"-AppleLanguages", @"(en)", @"-AppleLocale", @"en_US"];
  [app launch];

  XCUIElementQuery *items = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *title = items[@"Draft title"];
  XCUIElement *save = app.buttons[@"Save drafts"];
  XCTAssertTrue([title waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [title tap];
  NSPredicate *softwareKeyboardVisible = [NSPredicate predicateWithBlock:^BOOL(XCUIApplication *screen, NSDictionary *bindings) {
    return SoftwareKeyboardVisible(screen);
  }];
  XCTNSPredicateExpectation *softwareKeyboard = [[XCTNSPredicateExpectation alloc]
      initWithPredicate:softwareKeyboardVisible object:app];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[softwareKeyboard] timeout:12], XCTWaiterResultCompleted,
                 @"The simulator must show the onscreen keyboard before rotating. %@", app.debugDescription);
  XCUIElement *continueButton = app.buttons[@"Continue"];
  if (continueButton.exists) {
    [continueButton tap];
  }

  device.orientation = UIDeviceOrientationLandscapeLeft;
  NSPredicate *fits = [NSPredicate predicateWithBlock:^BOOL(NSString *label, NSDictionary *bindings) {
    XCUIElement *entry = items[label];
    return CGRectGetWidth(app.frame) > CGRectGetHeight(app.frame) &&
        [softwareKeyboardVisible evaluateWithObject:app] &&
        entry.exists && CGRectGetHeight(entry.frame) >= 28 && save.hittable &&
        CGRectGetMaxY(save.frame) <= CGRectGetMinY(app.keyboards.firstMatch.frame) + 1;
  }];
  XCTNSPredicateExpectation *titleFits = [[XCTNSPredicateExpectation alloc] initWithPredicate:fits object:@"Draft title"];
  XCTWaiterResult titleResult = [XCTWaiter waitForExpectations:@[titleFits] timeout:10];
  if (titleResult != XCTWaiterResultCompleted) {
    XCUIElement *keyboard = app.keyboards.firstMatch;
    XCTAttachment *screenshot = [XCTAttachment attachmentWithScreenshot:XCUIScreen.mainScreen.screenshot];
    screenshot.lifetime = XCTAttachmentLifetimeKeepAlways;
    [self addAttachment:screenshot];
    XCTFail(@"Landscape frames: window %@, keyboard %@, title %@, save %@. %@",
            NSStringFromCGRect(app.frame), keyboard.exists ? NSStringFromCGRect(keyboard.frame) : @"missing",
            items[@"Draft title"].exists ? NSStringFromCGRect(items[@"Draft title"].frame) : @"missing",
            save.exists ? NSStringFromCGRect(save.frame) : @"missing", app.debugDescription);
  }
  XCTAssertEqualObjects(items[@"Draft title"].value, @"Survey");

  XCUIElement *next = app.buttons[@"Next field"];
  XCTAssertTrue([next waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [next tap];
  XCUIElement *note = items[@"Review note"];
  [self expectationForPredicate:fits evaluatedWithObject:@"Review note" handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
  for (NSString *letter in @[@"a", @"i"]) {
    XCUIElement *key = [app.keyboards.firstMatch.keys matchingPredicate:
        [NSPredicate predicateWithFormat:@"label ==[c] %@", letter]].firstMatch;
    XCTAssertTrue([key waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
    [key tap];
  }
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value ==[c] %@", @"ai"]
           evaluatedWithObject:note handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [save tap];
  XCTAssertTrue([app.staticTexts[@"Keyboard dialog closed"] waitForExistenceWithTimeout:5]);
  XCTAssertFalse(save.exists);
}

- (void)testTouchScrollingKeepsTheHardwareKeyboardAccessoryAndEditorActive {
  self.continueAfterFailure = NO;
  SetSoftwareKeyboardAutomaticMinimizationInSimulator(YES);
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationLandscapeLeft;
  [self addTeardownBlock:^{ device.orientation = UIDeviceOrientationPortrait; }];
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--keyboard-layout", @"-AppleLanguages", @"(en)", @"-AppleLocale", @"en_US"];
  [app launch];

  XCUIElementQuery *items = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *title = items[@"Draft title"];
  XCTAssertTrue([title waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [title tap];
  XCUIElement *next = app.buttons[@"Next field"];
  XCTAssertTrue([next waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTSkipIf(SoftwareKeyboardVisible(app), @"This case requires the hardware keyboard with only its accessory toolbar visible.");
  XCUIElement *toolbar = app.toolbars.firstMatch;
  XCUIElement *save = app.buttons[@"Save drafts"];
  XCTAssertTrue([toolbar waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(next.hittable);
  XCTAssertGreaterThan(CGRectGetWidth(app.frame), CGRectGetHeight(app.frame));
  XCTAssertLessThanOrEqual(CGRectGetMaxY(save.frame), CGRectGetMinY(toolbar.frame) + 1,
      @"The save button should be above the accessory even with zero bottom safe-area inset.");
  XCUIElement *form = items[@"Keyboard setup form"];
  XCUIElement *code = items[@"Access code"];
  XCTAssertFalse(code.hittable, @"Access code should initially be below the scroll viewport.");
  [[form coordinateWithNormalizedOffset:CGVectorMake(0.88, 0.86)]
      pressForDuration:0.05
      thenDragToCoordinate:[form coordinateWithNormalizedOffset:CGVectorMake(0.88, 0.16)]];
  XCTAssertTrue(app.buttons[@"Next field"].hittable, @"Scrolling must keep the hardware editor's toolbar. %@", app.debugDescription);
  XCTAssertTrue([code waitForExistenceWithTimeout:5], @"The form should scroll. %@", app.debugDescription);
  XCTAssertTrue(code.hittable, @"The form should scroll to the access code. %@", app.debugDescription);
  XCTAssertFalse(SoftwareKeyboardVisible(app));
  [app.buttons[@"Next field"] tap];

  XCUIElement *note = items[@"Review note"];
  XCTAssertTrue([note waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [app typeKey:@"a" modifierFlags:XCUIKeyModifierNone];
  [app typeKey:@"i" modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value ==[c] %@", @"ai"]
           evaluatedWithObject:note handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [save tap];
  XCTAssertTrue([app.staticTexts[@"Keyboard dialog closed"] waitForExistenceWithTimeout:5]);
}

- (void)testTextSelectionScrollsWhileTheFingerHoldsAtTheEdge {
  self.continueAfterFailure = NO;
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--selection-scroll"];
  [app launch];
  XCUIElement *notes = [app descendantsMatchingType:XCUIElementTypeAny][@"Scrollable inspection notes"];
  XCTAssertTrue([notes waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Selected characters 0"].exists);
  XCUICoordinate *firstLine = [[notes coordinateWithNormalizedOffset:CGVectorMake(0, 0)]
      coordinateWithOffset:CGVectorMake(28, 14)];
  XCUICoordinate *bottom = [[notes coordinateWithNormalizedOffset:CGVectorMake(0, 1)]
      coordinateWithOffset:CGVectorMake(260, 42)];
  [firstLine pressForDuration:0.72 thenDragToCoordinate:bottom
    withVelocity:XCUIGestureVelocityFast thenHoldForDuration:1.45];
  XCTAssertTrue([app.staticTexts[@"End of notes selected"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCUIElement *count = [app.staticTexts matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", @"Selected characters "]].firstMatch;
  NSString *released = count.label;
  NSPredicate *changed = [NSPredicate predicateWithBlock:^BOOL(XCUIElement *label, NSDictionary *bindings) {
    return ![label.label isEqualToString:released];
  }];
  XCTestExpectation *remains = [self expectationForPredicate:changed evaluatedWithObject:count handler:nil];
  remains.inverted = YES;
  [self waitForExpectationsWithTimeout:0.6 handler:nil];
}

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
