#import <UIKit/UIKit.h>
#import <XCTest/XCTest.h>

@interface PocketTasksUITest : XCTestCase
@end

@implementation PocketTasksUITest

- (void)dismissKeyboardIntroductionInApp:(XCUIApplication *)app {
  // QuickPath can cover the keys when an English keyboard first opens.
  XCUIElement *introduction = app.buttons[@"Continue"];
  if ([introduction waitForExistenceWithTimeout:2]) {
    [introduction tap];
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"exists == false"]
             evaluatedWithObject:introduction handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
  }
}

- (void)tapKeyboardKey:(XCUIElement *)key inApp:(XCUIApplication *)app {
  NSPredicate *onScreen = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    XCUIElement *element = (XCUIElement *)object;
    return element.exists && CGRectContainsRect(app.frame, element.frame);
  }];
  [self expectationForPredicate:onScreen evaluatedWithObject:key handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [[key coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
}

- (XCUIElement *)visibleKeyboardInApp:(XCUIApplication *)app {
  CGRect screen = app.frame;
  __block XCUIElement *visible = nil;
  NSPredicate *onScreen = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    for (XCUIElement *keyboard in app.keyboards.allElementsBoundByIndex) {
      CGRect frame = keyboard.frame;
      if (!CGRectIsEmpty(frame) && CGRectContainsRect(screen, frame)) {
        visible = keyboard;
        return YES;
      }
    }
    return NO;
  }];
  [self expectationForPredicate:onScreen evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:8 handler:nil];
  return visible;
}

- (void)testKeyboardAndLandscapeTaskFlow {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *entry = [app descendantsMatchingType:XCUIElementTypeAny][@"New task"];
  XCUIElement *open = app.buttons[@"Open tasks"];
  XCUIElement *list = [app descendantsMatchingType:XCUIElementTypeAny][@"Open checklist"];
  XCTAssertTrue([entry waitForExistenceWithTimeout:15]);
  [entry tap];

  XCUIElement *keyboard = app.keyboards.firstMatch;
  XCTAssertTrue([keyboard waitForExistenceWithTimeout:5]);
  [self dismissKeyboardIntroductionInApp:app];
  NSPredicate *fitsAboveKeyboard = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return CGRectGetMaxY(list.frame) <= CGRectGetMinY(keyboard.frame) + 1;
  }];
  [self expectationForPredicate:fitsAboveKeyboard evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  NSPredicate *numberMode =
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"123", @"数字", @"Numbers", @"numbers"]];
  if (!keyboard.keys[@"1"].exists) {
    [self tapKeyboardKey:[keyboard.keys matchingPredicate:numberMode].firstMatch inApp:app];
  }
  [self tapKeyboardKey:keyboard.keys[@"1"] inApp:app];
  [self tapKeyboardKey:keyboard.keys[@"2"] inApp:app];
  [self tapKeyboardKey:keyboard.keys[@"3"] inApp:app];
  [app.buttons[@"Add"] tap];
  XCTAssertTrue([app.staticTexts[@"3 / 13"] waitForExistenceWithTimeout:5]);

  device.orientation = UIDeviceOrientationLandscapeLeft;
  NSPredicate *besideSummary = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return CGRectGetMinX(open.frame) > CGRectGetMaxX(entry.frame);
  }];
  [self expectationForPredicate:besideSummary evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCUICoordinate *firstTask =
      [list coordinateWithNormalizedOffset:CGVectorMake(0.4, 30.0 / CGRectGetHeight(list.frame))];
  [firstTask tap];
  XCTAssertTrue([app.staticTexts[@"4 / 13"] waitForExistenceWithTimeout:5]);
  [app.buttons[@"Completed tasks"] tap];
  XCUIElement *completed = [app descendantsMatchingType:XCUIElementTypeAny][@"Completed checklist"];
  XCTAssertTrue([completed waitForExistenceWithTimeout:5]);
  XCTAssertTrue(app.staticTexts[@"4 / 13"].exists);
  // The hardware driver checks Home and reactivation without XCTest's
  // instrumentation in the app process.
  device.orientation = UIDeviceOrientationPortrait;
}

- (void)testProgressAnimationPublishesItsFinalAccessibleValue {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *row = app.buttons[@"Check pump pressure"];
  XCTAssertTrue([row waitForExistenceWithTimeout:15]);
  XCUIElement *progress = app.otherElements[@"Round progress"];
  XCTAssertTrue([progress waitForExistenceWithTimeout:5]);
  XCTAssertEqualWithAccuracy([[progress value] doubleValue], 3.0, 0.001);

  [row tap];
  XCTAssertTrue([app.staticTexts[@"4 / 12"] waitForExistenceWithTimeout:5]);
  NSPredicate *finished = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    XCUIElement *element = (XCUIElement *)object;
    return fabs([[element value] doubleValue] - 4.0) < 0.001;
  }];
  [self expectationForPredicate:finished evaluatedWithObject:progress handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testCalendarExposesDatesAndPreservesTheDraftUntilSaved {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];

  XCUIElement *initial = app.buttons[@"Choose inspection date, 2024-02-28"];
  XCUIElement *saved = app.buttons[@"Choose inspection date, 2024-02-29"];
  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  [[initial coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  [app.buttons[@"Next month"] tap];
  XCTAssertTrue([elements[@"2024-03-01"] waitForExistenceWithTimeout:5]);
  XCTAssertEqualObjects(elements[@"2024-02-28"].value, @"1");
  [app.buttons[@"Next month"] tap];
  XCTAssertTrue([elements[@"2024-04-01"] waitForExistenceWithTimeout:5]);
  XCTAssertFalse(elements[@"2024-02-28"].exists);
  [app.buttons[@"Cancel"] tap];
  XCTAssertTrue([initial waitForExistenceWithTimeout:5]);

  for (NSNumber *confirm in @[@NO, @YES]) {
    [[initial coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
    XCUIElement *leapDay = elements[@"2024-02-29"];
    XCUIElement *weekend = elements[@"2024-03-02"];
    XCTAssertTrue([leapDay waitForExistenceWithTimeout:5]);
    XCTAssertTrue(leapDay.enabled);
    XCTAssertTrue(weekend.exists);
    XCTAssertFalse(weekend.enabled);
    XCTAssertTrue(app.buttons[@"Next month"].exists);
    XCTAssertFalse(initial.exists);

    [leapDay tap];
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"1"]
             evaluatedWithObject:leapDay handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
    XCUIElement *response = app.buttons[confirm.boolValue ? @"Save date" : @"Cancel"];
    [[response coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
    XCTAssertTrue([(confirm.boolValue ? saved : initial) waitForExistenceWithTimeout:5]);
    XCTAssertFalse(leapDay.exists);
  }
}

- (void)testLandscapeTimeWheelDragAndSave {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  XCUIElement *initial = app.buttons[@"Choose inspection time, 12:30:41"];
  [initial tap];

  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *minute = elements[@"Minute"];
  XCTAssertTrue([minute waitForExistenceWithTimeout:5]);
  XCTAssertEqualObjects(minute.value, @"30");
  device.orientation = UIDeviceOrientationLandscapeLeft;
  NSPredicate *landscape = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    XCUIApplication *application = (XCUIApplication *)object;
    return CGRectGetWidth(application.frame) > CGRectGetHeight(application.frame);
  }];
  [self expectationForPredicate:landscape evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(CGRectContainsRect(app.frame, minute.frame));
  XCTAssertTrue(elements[@"Hour"].exists);
  XCTAssertTrue(elements[@"Second"].exists);
  XCTAssertTrue(elements[@"AM/PM"].exists);
  XCUICoordinate *start = [minute coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.62)];
  XCUICoordinate *end = [minute coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.23)];
  [start pressForDuration:0.08 thenDragToCoordinate:end withVelocity:XCUIGestureVelocitySlow
      thenHoldForDuration:0.25];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"32"]
           evaluatedWithObject:minute handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCUIElement *save = app.buttons[@"Save time"];
  XCTAssertTrue(CGRectContainsRect(app.frame, save.frame));
  XCTAssertTrue(save.enabled);
  [save tap];
  XCTAssertTrue([app.buttons[@"Choose inspection time, 12:32:41"] waitForExistenceWithTimeout:5]);
  XCTAssertFalse(minute.exists);
  device.orientation = UIDeviceOrientationPortrait;
}

- (void)testContactFieldsUseEmailURLAndTelephoneKeyboards {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  [app.buttons[@"Contact details"] tap];

  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *email = elements[@"Contact email"];
  XCTAssertTrue([email waitForExistenceWithTimeout:5]);
  [email tap];
  [self dismissKeyboardIntroductionInApp:app];
  XCUIElement *keyboard = [self visibleKeyboardInApp:app];
  XCTAssertTrue([keyboard.keys[@"@"] waitForExistenceWithTimeout:5]);
  [keyboard.keys[@"@"] tap];
  XCTAssertEqualObjects(email.value, @"@");
  NSPredicate *nextLabel = [NSPredicate predicateWithFormat:@"label ==[c] %@", @"next"];
  XCUIElement *next = [keyboard.buttons matchingPredicate:nextLabel].firstMatch;
  XCTAssertTrue([next waitForExistenceWithTimeout:5]);
  [next tap];
  XCTAssertTrue(keyboard.keys[@"@"].exists);
  XCTAssertFalse(app.staticTexts[@"Email: @"].exists);
  [keyboard.keys[@"a"] tap];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"@a"]
           evaluatedWithObject:email handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [next tap];
  XCTAssertTrue([app.staticTexts[@"Email: @a"] waitForExistenceWithTimeout:5]);

  XCUIElement *telephone = elements[@"Contact telephone"];
  XCUIElement *toolbarNext = app.toolbars.buttons[@"Next"];
  XCTAssertTrue([toolbarNext waitForExistenceWithTimeout:5]);
  keyboard = [self visibleKeyboardInApp:app];
  // UIKit exposes the telephone keypad's +*# switch as Shift.
  XCTAssertTrue([keyboard.keys[@"Shift"] waitForExistenceWithTimeout:5]);
  [keyboard.keys[@"1"] tap];
  XCTAssertEqualObjects(telephone.value, @"1");
  [[toolbarNext coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  XCTAssertTrue(keyboard.exists);
  XCTAssertFalse(app.staticTexts[@"Telephone: 1"].exists);
  [keyboard.keys[@"2"] tap];
  [keyboard.keys[@"3"] tap];
  XCTAssertEqualObjects(telephone.value, @"123");
  [[toolbarNext coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  XCTAssertTrue([app.staticTexts[@"Telephone: 123"] waitForExistenceWithTimeout:5]);

  XCUIElement *website = elements[@"Contact website"];
  keyboard = [self visibleKeyboardInApp:app];
  XCTAssertTrue([keyboard.keys[@"/"] waitForExistenceWithTimeout:5]);
  [keyboard.keys[@"/"] tap];
  XCTAssertEqualObjects(website.value, @"/");
  NSPredicate *doneLabel = [NSPredicate predicateWithFormat:@"label ==[c] %@", @"done"];
  XCUIElement *done = [keyboard.buttons matchingPredicate:doneLabel].firstMatch;
  XCTAssertTrue([done waitForExistenceWithTimeout:5]);
  [done tap];
  XCTAssertTrue([app.staticTexts[@"Website: /"] waitForExistenceWithTimeout:5]);
  NSPredicate *keyboardHidden = [NSPredicate predicateWithFormat:@"exists == false"];
  [self expectationForPredicate:keyboardHidden evaluatedWithObject:app.keyboards.firstMatch handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testLandscapeReadingStaysAboveTheNumericKeyboard {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *field = elements[@"Validated reading"];
  [field tap];
  XCUIElement *keyboard = app.keyboards.firstMatch;
  XCUIElement *toolbar = app.toolbars.firstMatch;
  XCTAssertTrue([keyboard waitForExistenceWithTimeout:5]);
  XCTAssertTrue([toolbar waitForExistenceWithTimeout:5]);
  [app typeKey:@"4" modifierFlags:XCUIKeyModifierNone];
  device.orientation = UIDeviceOrientationLandscapeLeft;
  NSPredicate *fits = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    XCUIElement *editor = (XCUIElement *)object;
    CGFloat keyboardTop = MIN(CGRectGetMinY(keyboard.frame), CGRectGetMinY(toolbar.frame));
    return CGRectGetWidth(app.frame) > CGRectGetHeight(app.frame) && keyboard.exists && toolbar.exists
        && editor.exists && CGRectGetHeight(editor.frame) >= 37.5
        && CGRectGetMaxY(editor.frame) <= keyboardTop + 1;
  }];
  [self expectationForPredicate:fits evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [app typeKey:@"5" modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"45"]
           evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  device.orientation = UIDeviceOrientationPortrait;
}

- (void)waitForField:(XCUIElement *)field aboveKeyboard:(XCUIElement *)keyboard
              toolbar:(XCUIElement *)toolbar {
  NSPredicate *fits = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    CGRect editor = field.frame;
    CGFloat top = CGRectGetMinY(keyboard.frame);
    if (toolbar != nil && toolbar.exists) {
      top = MIN(top, CGRectGetMinY(toolbar.frame));
    }
    return field.exists && keyboard.exists && CGRectGetHeight(editor) >= 37.5
        && CGRectGetMaxY(editor) <= top + 1;
  }];
  [self expectationForPredicate:fits evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testLandscapeContactFieldsScrollWithNextAndStayEditable {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  XCUIDevice *device = [XCUIDevice sharedDevice];
  device.orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  [app.buttons[@"Contact details"] tap];
  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *email = elements[@"Contact email"];
  XCTAssertTrue([email waitForExistenceWithTimeout:5]);
  [email tap];
  XCTAssertTrue([app.keyboards.firstMatch waitForExistenceWithTimeout:5]);
  [self dismissKeyboardIntroductionInApp:app];
  [app typeKey:@"a" modifierFlags:XCUIKeyModifierNone];
  [app typeKey:@"b" modifierFlags:XCUIKeyModifierNone];
  XCTAssertEqualObjects(email.value, @"ab");

  device.orientation = UIDeviceOrientationLandscapeLeft;
  NSPredicate *landscape = [NSPredicate predicateWithBlock:^BOOL(XCUIApplication *application, NSDictionary *bindings) {
    CGRect screen = application.frame;
    return CGRectGetWidth(screen) > CGRectGetHeight(screen);
  }];
  [self expectationForPredicate:landscape evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCUIElement *keyboard = [self visibleKeyboardInApp:app];
  [self waitForField:email aboveKeyboard:keyboard toolbar:nil];
  XCTAssertEqualObjects(email.value, @"ab");
  NSPredicate *nextLabel = [NSPredicate predicateWithFormat:@"label ==[c] %@", @"next"];
  XCUIElement *next = [keyboard.buttons matchingPredicate:nextLabel].firstMatch;
  XCTAssertTrue([next waitForExistenceWithTimeout:5]);
  [next tap];

  XCUIElement *toolbarNext = app.toolbars.buttons[@"Next"];
  XCTAssertTrue([toolbarNext waitForExistenceWithTimeout:5]);
  keyboard = [self visibleKeyboardInApp:app];
  XCUIElement *telephone = elements[@"Contact telephone"];
  [self waitForField:telephone aboveKeyboard:keyboard toolbar:toolbarNext];
  for (NSString *digit in @[@"1", @"2", @"3"]) {
    [app typeKey:digit modifierFlags:XCUIKeyModifierNone];
  }
  XCTAssertEqualObjects(telephone.value, @"123");
  [[toolbarNext coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];

  XCUIElement *website = elements[@"Contact website"];
  keyboard = [self visibleKeyboardInApp:app];
  [self waitForField:website aboveKeyboard:keyboard toolbar:nil];
  [app typeKey:@"/" modifierFlags:XCUIKeyModifierNone];
  XCTAssertEqualObjects(website.value, @"/");
  NSPredicate *doneLabel = [NSPredicate predicateWithFormat:@"label ==[c] %@", @"done"];
  XCUIElement *done = [keyboard.buttons matchingPredicate:doneLabel].firstMatch;
  XCTAssertTrue([done waitForExistenceWithTimeout:5]);
  [done tap];

  XCTAssertTrue([app.staticTexts[@"Website: /"] waitForExistenceWithTimeout:5]);
  XCTAssertEqualObjects(email.value, @"ab");
  XCTAssertEqualObjects(telephone.value, @"123");
  [app.buttons[@"Close contact details"] tap];
  XCTAssertTrue([app.buttons[@"Contact details"] waitForExistenceWithTimeout:5]);
  device.orientation = UIDeviceOrientationPortrait;
}

- (void)testTaskSwipeActionsKeepTheTaskUntilConfirmation {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *row = app.buttons[@"Check pump pressure"];
  XCUIElement *complete = app.buttons[@"Complete Check pump pressure"];
  XCTAssertTrue([row waitForExistenceWithTimeout:15]);
  [row swipeLeft];
  XCTAssertTrue([complete waitForExistenceWithTimeout:5]);
  XCTAssertTrue(app.staticTexts[@"3 / 12"].exists);

  [complete swipeRight];
  NSPredicate *closed = [NSPredicate predicateWithFormat:@"exists == false"];
  [self expectationForPredicate:closed evaluatedWithObject:complete handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(app.staticTexts[@"3 / 12"].exists);

  [row swipeLeft];
  XCTAssertTrue([complete waitForExistenceWithTimeout:5]);
  [complete tap];
  XCTAssertTrue([app.staticTexts[@"4 / 12"] waitForExistenceWithTimeout:5]);
  [app.buttons[@"Completed tasks"] tap];
  XCTAssertTrue([row waitForExistenceWithTimeout:5]);
  [row swipeLeft];
  XCUIElement *reopen = app.buttons[@"Reopen Check pump pressure"];
  XCTAssertTrue([reopen waitForExistenceWithTimeout:5]);
  [reopen tap];
  XCTAssertTrue([app.staticTexts[@"3 / 12"] waitForExistenceWithTimeout:5]);
}

- (void)testHardwareKeyboardEditsTextAndMovesToTheNextControl {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *entry = [app descendantsMatchingType:XCUIElementTypeAny][@"New task"];
  XCTAssertTrue([entry waitForExistenceWithTimeout:15]);
  [entry tap];
  XCTAssertTrue([app.keyboards.firstMatch waitForExistenceWithTimeout:5]);
  for (NSString *key in @[@"1", @"2", @"3"]) {
    [app typeKey:key modifierFlags:XCUIKeyModifierNone];
  }
  [app typeKey:XCUIKeyboardKeyLeftArrow modifierFlags:XCUIKeyModifierNone];
  [app typeKey:@"4" modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"1243"]
           evaluatedWithObject:entry handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  [app typeKey:@"a" modifierFlags:XCUIKeyModifierCommand];
  [app typeKey:@"9" modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"9"]
           evaluatedWithObject:entry handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  [app typeKey:XCUIKeyboardKeyTab modifierFlags:XCUIKeyModifierNone];
  XCTAssertEqualObjects(entry.value, @"9");
  NSPredicate *keyboardHidden = [NSPredicate predicateWithFormat:@"exists == false"];
  [self expectationForPredicate:keyboardHidden evaluatedWithObject:app.keyboards.firstMatch handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(app.staticTexts[@"3 / 12"].exists);
}

- (void)testNumberStepsFromTheVisibleDraftWithKeyboardAndTouch {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  XCUIElement *count = [app descendantsMatchingType:XCUIElementTypeAny][@"Sample count"];
  [[count coordinateWithNormalizedOffset:CGVectorMake(0.35, 0.5)] tap];
  XCTAssertTrue([app.keyboards.firstMatch waitForExistenceWithTimeout:5]);

  [app typeKey:@"a" modifierFlags:XCUIKeyModifierCommand];
  [app typeKey:@"7" modifierFlags:XCUIKeyModifierNone];
  XCTAssertEqualObjects(count.value, @"0");
  [app typeKey:XCUIKeyboardKeyUpArrow modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"8"]
           evaluatedWithObject:count handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  [app typeKey:@"a" modifierFlags:XCUIKeyModifierCommand];
  [app typeKey:@"3" modifierFlags:XCUIKeyModifierNone];
  // Hit the upper half of the right-side stepper while the keypad is open.
  [[count coordinateWithNormalizedOffset:CGVectorMake(0.93, 0.25)] tap];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"4"]
           evaluatedWithObject:count handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testHardwareKeyboardNavigatesTaskActions {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCTAssertTrue([app.buttons[@"Check pump pressure"] waitForExistenceWithTimeout:15]);
  XCUIElement *complete = app.buttons[@"Complete Check pump pressure"];
  for (NSUInteger index = 0; index < 12 && !complete.exists; index++) {
    [app typeKey:XCUIKeyboardKeyTab modifierFlags:XCUIKeyModifierNone];
    [app typeKey:XCUIKeyboardKeyLeftArrow modifierFlags:XCUIKeyModifierNone];
  }
  XCTAssertTrue([complete waitForExistenceWithTimeout:5]);
  [app typeKey:XCUIKeyboardKeyRightArrow modifierFlags:XCUIKeyModifierNone];
  NSPredicate *closed = [NSPredicate predicateWithFormat:@"exists == false"];
  [self expectationForPredicate:closed evaluatedWithObject:complete handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  [app typeKey:XCUIKeyboardKeyTab modifierFlags:XCUIKeyModifierNone];
  [app typeKey:XCUIKeyboardKeyLeftArrow modifierFlags:XCUIKeyModifierNone];
  XCTAssertTrue([complete waitForExistenceWithTimeout:5]);
  [app typeKey:XCUIKeyboardKeyRightArrow modifierFlags:XCUIKeyModifierNone];
  [self expectationForPredicate:closed evaluatedWithObject:complete handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(app.staticTexts[@"3 / 12"].exists);
}

- (void)testTaskContextMenuOnTouch {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *row = app.buttons[@"Check pump pressure"];
  XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *mark = elements[@"Mark complete"];
  XCTAssertTrue([row waitForExistenceWithTimeout:15]);
  [row pressForDuration:0.7];
  XCTAssertTrue([mark waitForExistenceWithTimeout:5]);
  XCTAssertTrue(app.staticTexts[@"3 / 12"].exists);
  [mark tap];
  XCTAssertTrue([app.staticTexts[@"4 / 12"] waitForExistenceWithTimeout:5]);

  [app.buttons[@"Completed tasks"] tap];
  XCTAssertTrue([row waitForExistenceWithTimeout:5]);
  [row pressForDuration:0.7];
  XCUIElement *reopen = elements[@"Reopen task"];
  XCTAssertTrue([reopen waitForExistenceWithTimeout:5]);
  [reopen tap];
  XCTAssertTrue([app.staticTexts[@"3 / 12"] waitForExistenceWithTimeout:5]);
}

- (void)testNotesKeyboardAndAccessibleCategoryOptions {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *showNotes = app.buttons[@"Open field notes"];
  XCTAssertTrue([showNotes waitForExistenceWithTimeout:15]);
  [showNotes tap];

  XCUIElement *notes = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection notes"];
  [notes tap];
  XCUIElement *keyboard = app.keyboards.firstMatch;
  XCTAssertTrue([keyboard waitForExistenceWithTimeout:5]);
  [self dismissKeyboardIntroductionInApp:app];
  NSPredicate *newline =
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"Return", @"return", @"换行"]];
  // Use single-letter English words so Return does not autocorrect the text.
  XCUIElement *first = [keyboard.keys matchingPredicate:[NSPredicate predicateWithFormat:@"label ==[c] %@", @"a"]].firstMatch;
  XCUIElement *lineBreak = [[keyboard descendantsMatchingType:XCUIElementTypeAny] matchingPredicate:newline].firstMatch;
  XCUIElement *second = [keyboard.keys matchingPredicate:[NSPredicate predicateWithFormat:@"label ==[c] %@", @"i"]].firstMatch;
  [first tap];
  [lineBreak tap];
  [second tap];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value ==[c] %@", @"a\ni"]
           evaluatedWithObject:notes
                       handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCUIElement *category = [app descendantsMatchingType:XCUIElementTypeAny][@"Note category"];
  [[category coordinateWithNormalizedOffset:CGVectorMake(0.94, 0.5)] tap];
  XCUIElement *urgent = [app descendantsMatchingType:XCUIElementTypeAny][@"Urgent"];
  XCTAssertTrue([urgent waitForExistenceWithTimeout:5]);
  [urgent tap];
  XCTAssertEqualObjects(category.value, @"Urgent");
  XCTAssertEqualObjects([notes.value lowercaseString], @"a\ni");
}

@end
