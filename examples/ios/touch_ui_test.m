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
  XCUIElement *previousField = app.toolbars.buttons[@"Previous field"];
  XCUIElement *nextField = app.toolbars.buttons[@"Next field"];
  XCTAssertTrue([previousField waitForExistenceWithTimeout:5]);
  XCTAssertFalse(previousField.enabled);
  XCTAssertTrue(nextField.enabled);
  [self tapKeyboardKey:keyboard.keys[@"@"] inApp:app];
  XCTAssertEqualObjects(email.value, @"@");
  NSPredicate *nextLabel = [NSPredicate predicateWithFormat:@"label ==[c] %@", @"next"];
  XCUIElement *next = [keyboard.buttons matchingPredicate:nextLabel].firstMatch;
  XCTAssertTrue([next waitForExistenceWithTimeout:5]);
  [next tap];
  XCTAssertTrue(keyboard.keys[@"@"].exists);
  XCTAssertFalse(app.staticTexts[@"Email: @"].exists);
  [self tapKeyboardKey:keyboard.keys[@"a"] inApp:app];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"@a"]
           evaluatedWithObject:email handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [next tap];
  XCTAssertTrue([app.staticTexts[@"Email: @a"] waitForExistenceWithTimeout:5]);

  XCUIElement *telephone = elements[@"Contact telephone"];
  XCUIElement *toolbarNext = app.toolbars.buttons[@"Next"];
  XCTAssertTrue([toolbarNext waitForExistenceWithTimeout:5]);
  keyboard = [self visibleKeyboardInApp:app];
  // The iPhone keypad and the iPad punctuation keyboard name the switch differently.
  NSString *symbolsLabel = UIDevice.currentDevice.userInterfaceIdiom == UIUserInterfaceIdiomPhone
      ? @"Shift" : @"symbols";
  XCUIElement *symbols = keyboard.keys[symbolsLabel].firstMatch;
  XCTAssertTrue([symbols waitForExistenceWithTimeout:5]);
  [self tapKeyboardKey:keyboard.keys[@"1"] inApp:app];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"1"]
           evaluatedWithObject:telephone handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(previousField.enabled);
  [previousField tap];
  XCTAssertTrue(symbols.exists);
  XCTAssertEqualObjects(telephone.value, @"1");
  XCTAssertFalse(app.staticTexts[@"Telephone: 1"].exists);
  [[toolbarNext coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  XCTAssertTrue(keyboard.exists);
  XCTAssertFalse(app.staticTexts[@"Telephone: 1"].exists);
  for (NSString *draft in @[@"12", @"123"]) {
    [self tapKeyboardKey:keyboard.keys[[draft substringFromIndex:draft.length - 1]] inApp:app];
    NSPredicate *updated = [NSPredicate predicateWithFormat:@"value == %@", draft];
    XCTNSPredicateExpectation *expectation = [[XCTNSPredicateExpectation alloc] initWithPredicate:updated object:telephone];
    XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:5], XCTWaiterResultCompleted,
                   @"Expected telephone draft %@, found %@", draft, telephone.value);
  }
  [previousField tap];
  XCTAssertTrue([app.staticTexts[@"Telephone: 123"] waitForExistenceWithTimeout:5]);
  keyboard = [self visibleKeyboardInApp:app];
  XCTAssertTrue([keyboard.keys[@"@"] waitForExistenceWithTimeout:5]);
  XCTAssertFalse(previousField.enabled);
  [nextField tap];
  XCTAssertTrue([toolbarNext waitForExistenceWithTimeout:5]);
  XCTAssertEqualObjects(telephone.value, @"123");
  [[toolbarNext coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  XCTAssertTrue([app.staticTexts[@"Telephone: 123"] waitForExistenceWithTimeout:5]);

  XCUIElement *website = elements[@"Contact website"];
  keyboard = [self visibleKeyboardInApp:app];
  XCTAssertTrue([keyboard.keys[@"/"] waitForExistenceWithTimeout:5]);
  XCTAssertTrue(previousField.enabled);
  XCTAssertFalse(nextField.enabled);
  [self tapKeyboardKey:keyboard.keys[@"/"] inApp:app];
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

- (XCUIApplication *)openPhotoGalleryWithTwoFixtures {
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--choose-photos"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *choose = app.buttons[@"Choose photos"];
  XCTAssertTrue([choose waitForExistenceWithTimeout:15]);
  [choose tap];

  XCUIElement *cancel = app.buttons[@"Cancel"];
  XCTAssertTrue([cancel waitForExistenceWithTimeout:10], @"%@", app.debugDescription);
  XCUIElement *notice = [app.otherElements matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", @"Private Access to Photos"]].firstMatch;
  if ([notice waitForExistenceWithTimeout:3]) {
    XCUIElement *closeNotice = notice.buttons[@"Close"];
    XCTAssertTrue(closeNotice.exists, @"%@", app.debugDescription);
    [[closeNotice coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"exists == NO"]
             evaluatedWithObject:notice handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
  }
  NSPredicate *photoLabel = [NSPredicate predicateWithFormat:@"label BEGINSWITH[c] %@", @"Photo,"];
  XCUIElementQuery *photos = [app.images matchingPredicate:photoLabel];
  XCTAssertTrue([photos.firstMatch waitForExistenceWithTimeout:10], @"%@", app.debugDescription);
  NSArray<XCUIElement *> *fixtures = photos.allElementsBoundByIndex;
  XCTAssertGreaterThanOrEqual(fixtures.count, 2UL, @"%@", app.debugDescription);
  [[fixtures.firstObject coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  [[fixtures[1] coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  XCUIElement *add = [app.buttons matchingPredicate:[NSPredicate predicateWithFormat:@"label BEGINSWITH[c] %@", @"Add"]].firstMatch;
  XCUIElement *selected = [[app descendantsMatchingType:XCUIElementTypeAny] matchingPredicate:
      [NSPredicate predicateWithFormat:@"label == %@", @"Show Selected (2)"]].firstMatch;
  XCTAssertTrue([selected waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"exists == YES AND enabled == YES AND hittable == YES"]
           evaluatedWithObject:add handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [[add coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];

  XCTAssertTrue([app.staticTexts[@"Imported 2 photos"] waitForExistenceWithTimeout:15],
                @"%@", app.debugDescription);
  XCUIElement *firstPreview = app.images[@"Photo preview 1"];
  XCTAssertTrue([firstPreview waitForExistenceWithTimeout:5]);
  XCTAssertTrue(app.staticTexts[@"Photo 1 of 2"].exists);
  NSString *png = @"PNG · 96 × 64 px";
  NSString *heic = @"HEIC · 64 × 96 px";
  BOOL firstIsPNG = app.staticTexts[png].exists;
  XCTAssertTrue(firstIsPNG || app.staticTexts[heic].exists,
                @"%@", [app.staticTexts.allElementsBoundByIndex valueForKey:@"label"]);
  [firstPreview swipeLeft];
  XCUIElement *secondPreview = app.images[@"Photo preview 2"];
  XCTAssertTrue([secondPreview waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Photo 2 of 2"].exists);
  XCTAssertTrue(app.staticTexts[firstIsPNG ? heic : png].exists,
                @"%@", [app.staticTexts.allElementsBoundByIndex valueForKey:@"label"]);
  return app;
}

- (void)confirmPocketTasksExportDirectoryInApp:(XCUIApplication *)app {
  XCUIElement *confirm = [app.buttons matchingPredicate:
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"Open", @"Done"]]].firstMatch;
  XCTAssertTrue([confirm waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCUIElement *folderTitle = [app.buttons matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", @"Pocket Tasks,"]].firstMatch;
  if (!folderTitle.exists) {
    XCUIElementQuery *elements = [app descendantsMatchingType:XCUIElementTypeAny];
    XCUIElement *pocketTasks = app.cells[@"Pocket Tasks, Container"].firstMatch;
    XCUIElement *onDevice = [elements matchingPredicate:
        [NSPredicate predicateWithFormat:@"label IN %@", @[@"On My iPhone", @"On My iPad"]]].firstMatch;
    if (!pocketTasks.exists) {
      XCUIElement *browse = app.buttons[@"Browse"];
      if (!onDevice.exists && browse.exists) {
        [[browse coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
      }
      XCTAssertTrue([onDevice waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
      [onDevice tap];
    }
    XCTAssertTrue([pocketTasks waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
    [[pocketTasks coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.35)] tap];
  }
  XCTAssertTrue([folderTitle waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
           evaluatedWithObject:confirm handler:nil];
  [self waitForExpectationsWithTimeout:8 handler:nil];
  [[confirm coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
}

- (void)testPhotoPickerExportsBothOriginalsAndKeepsThePreviewOnCancel {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [self openPhotoGalleryWithTwoFixtures];
  XCUIElement *secondPreview = app.images[@"Photo preview 2"];
  XCUIElement *saveAll = app.buttons[@"Save all"];
  if (UIDevice.currentDevice.userInterfaceIdiom == UIUserInterfaceIdiomPad) {
    [XCUIDevice sharedDevice].orientation = UIDeviceOrientationLandscapeLeft;
    NSPredicate *fits = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
      CGRect frame = app.frame;
      return CGRectGetWidth(frame) > CGRectGetHeight(frame) && secondPreview.exists &&
             saveAll.exists && CGRectContainsRect(frame, saveAll.frame);
    }];
    [self expectationForPredicate:fits evaluatedWithObject:app handler:nil];
    [self waitForExpectationsWithTimeout:8 handler:nil];
    XCTAssertTrue(app.staticTexts[@"Photo 2 of 2"].exists);
  }
  void (^openExport)(void) = ^{
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
             evaluatedWithObject:saveAll handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
    [[saveAll coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  };
  openExport();
  XCUIElement *cancel = app.buttons[@"Cancel"];
  XCTAssertTrue([cancel waitForExistenceWithTimeout:10], @"%@", app.debugDescription);
  [cancel tap];
  XCTAssertTrue([app.staticTexts[@"Save canceled"] waitForExistenceWithTimeout:8]);
  XCTAssertTrue(secondPreview.exists);
  XCTAssertTrue(saveAll.enabled);

  openExport();
  [self confirmPocketTasksExportDirectoryInApp:app];
  XCTAssertTrue([app.staticTexts[@"2 originals saved"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);
  XCTAssertTrue(secondPreview.exists);

  openExport();
  [self confirmPocketTasksExportDirectoryInApp:app];
  XCUIElement *choose = app.buttons[@"Choose photos"];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
           evaluatedWithObject:choose handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(app.staticTexts[@"2 originals saved"].exists);
  [choose tap];
  XCTAssertTrue([cancel waitForExistenceWithTimeout:10], @"%@", app.debugDescription);
  [cancel tap];
  XCTAssertTrue([app.staticTexts[@"Photo selection canceled"] waitForExistenceWithTimeout:5]);
  XCTAssertTrue(secondPreview.exists);
  XCTAssertTrue(app.staticTexts[@"Photo 2 of 2"].exists);
  XCTAssertTrue(choose.enabled);
}

- (void)testPhotoPickerSharesOriginalsAndKeepsThePreviewOnCancel {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [self openPhotoGalleryWithTwoFixtures];
  XCUIElement *share = app.buttons[@"Share originals"];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
           evaluatedWithObject:share handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [share tap];
  // The system's remote activity view does not expose its item labels to the app query.
  XCUIElement *activities = app.otherElements[@"ActivityListView"].firstMatch;
  XCTAssertTrue([activities waitForExistenceWithTimeout:12], @"%@", app.debugDescription);
  XCTAssertFalse(share.enabled);
  XCTAttachment *sheet = [XCTAttachment attachmentWithScreenshot:XCUIScreen.mainScreen.screenshot];
  sheet.name = @"Photo share sheet";
  sheet.lifetime = XCTAttachmentLifetimeKeepAlways;
  [self addAttachment:sheet];
  [[app coordinateWithNormalizedOffset:CGVectorMake(0.03, 0.08)] tap];
  XCTAssertTrue([app.staticTexts[@"Share canceled"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);
  XCTAssertTrue(app.images[@"Photo preview 2"].exists);
  XCTAssertTrue(app.staticTexts[@"Photo 2 of 2"].exists);
  XCTAssertTrue(share.enabled);
}

- (void)testListDragScrollsAtTheEdgeAndDropsOnAnInitiallyHiddenRow {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--reorder-list"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *pump = app.buttons[@"Pump pressure"];
  XCTAssertTrue([pump waitForExistenceWithTimeout:15]);
  [pump tap];
  XCUIElement *marked = app.buttons[@"Pump pressure · marked"];
  XCTAssertTrue([marked waitForExistenceWithTimeout:5]);
  XCTAssertFalse(app.buttons[@"Cooling fan"].exists);
  XCUIElement *queue = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection queue"];
  XCTAssertTrue(queue.exists);
  CGFloat distance = CGRectGetMaxY(queue.frame) - 3.0 - CGRectGetMidY(marked.frame);
  XCUICoordinate *origin = [marked coordinateWithNormalizedOffset:CGVectorMake(0.42, 0.5)];
  XCUICoordinate *edge = [origin coordinateWithOffset:CGVectorMake(0.0, distance)];
  [origin pressForDuration:0.7 thenDragToCoordinate:edge withVelocity:XCUIGestureVelocitySlow
         thenHoldForDuration:1.1];
  XCTAssertTrue([app.staticTexts[@"Moves: 1 · Selected row: 7"] waitForExistenceWithTimeout:5],
                @"%@", [app.staticTexts.allElementsBoundByIndex valueForKey:@"label"]);
  XCTAssertFalse(app.staticTexts[@"Drop to place the inspection"].exists);
  XCTAssertTrue(marked.exists);
  XCTAssertLessThan(CGRectGetMinY(app.buttons[@"Cooling fan"].frame), CGRectGetMinY(marked.frame));
}

- (void)testListRowsScrollQuicklyAndReorderAfterAHold {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--reorder-list"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *pump = app.buttons[@"Pump pressure"];
  XCTAssertTrue([pump waitForExistenceWithTimeout:15]);
  [pump tap];
  XCUIElement *marked = app.buttons[@"Pump pressure · marked"];
  XCUIElement *water = app.buttons[@"Water temperature"];
  XCTAssertTrue([marked waitForExistenceWithTimeout:5]);
  XCTAssertLessThan(CGRectGetMinY(marked.frame), CGRectGetMinY(water.frame));

  XCUICoordinate *from = [marked coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  XCUICoordinate *to = [water coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  [from pressForDuration:0.7 thenDragToCoordinate:to withVelocity:XCUIGestureVelocitySlow
       thenHoldForDuration:0.0];
  XCTAssertTrue([app.staticTexts[@"Moves: 1 · Selected row: 3"] waitForExistenceWithTimeout:5]);
  XCTAssertTrue(marked.exists);
  XCTAssertGreaterThan(CGRectGetMinY(marked.frame), CGRectGetMinY(water.frame));

  [app.buttons[@"Move Pump pressure up"] tap];
  XCTAssertTrue([app.staticTexts[@"Moves: 2 · Selected row: 2"] waitForExistenceWithTimeout:5],
                @"%@", [app.staticTexts.allElementsBoundByIndex valueForKey:@"label"]);
  XCTAssertLessThan(CGRectGetMinY(marked.frame), CGRectGetMinY(water.frame));

  XCUIElement *lighting = app.buttons[@"Emergency lighting"];
  XCTAssertFalse(lighting.exists);
  from = [water coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  to = [from coordinateWithOffset:CGVectorMake(0.0, -100.0)];
  [from pressForDuration:0.05 thenDragToCoordinate:to withVelocity:XCUIGestureVelocityFast
       thenHoldForDuration:0.0];
  XCTAssertTrue([lighting waitForExistenceWithTimeout:5]);
  XCTAssertTrue(app.staticTexts[@"Moves: 2 · Selected row: 2"].exists);
}

- (void)testReadingScrollDismissesTheKeyboardAndKeepsTheDraft {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *readings = app.buttons[@"Open readings"];
  XCTAssertTrue([readings waitForExistenceWithTimeout:15]);
  [readings tap];
  XCUIElement *field = [app descendantsMatchingType:XCUIElementTypeAny][@"Validated reading"];
  [field tap];
  XCUIElement *keyboard = [self visibleKeyboardInApp:app];
  XCUIElement *toolbar = app.toolbars.firstMatch;
  XCTAssertTrue([toolbar waitForExistenceWithTimeout:5]);
  [self tapKeyboardKey:keyboard.keys[@"4"] inApp:app];
  XCTAssertEqualObjects(field.value, @"4");

  XCUICoordinate *start = [field coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  XCUICoordinate *end = [start coordinateWithOffset:CGVectorMake(0.0, -65.0)];
  [start pressForDuration:0.05 thenDragToCoordinate:end withVelocity:XCUIGestureVelocitySlow
     thenHoldForDuration:0.0];
  NSPredicate *hidden = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return !app.keyboards.firstMatch.exists && !toolbar.exists;
  }];
  [self expectationForPredicate:hidden evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqualObjects(field.value, @"4");

  [field tap];
  keyboard = [self visibleKeyboardInApp:app];
  XCTAssertEqualObjects(field.value, @"4");
  [self tapKeyboardKey:keyboard.keys[@"5"] inApp:app];
  XCTAssertEqualObjects(field.value, @"45");
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

- (void)testUIKitEditMenuCutsUndoesAndKeepsReadOnlyCommands {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *showNotes = app.buttons[@"Open field notes"];
  XCTAssertTrue([showNotes waitForExistenceWithTimeout:15]);
  [showNotes tap];
  XCUIElement *notes = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection notes"];
  [notes tap];
  XCUIElement *keyboard = [self visibleKeyboardInApp:app];
  [self dismissKeyboardIntroductionInApp:app];
  XCUIElement *letter = [keyboard.keys matchingPredicate:
      [NSPredicate predicateWithFormat:@"label ==[c] %@", @"a"]].firstMatch;
  [self tapKeyboardKey:letter inApp:app];
  NSPredicate *hasText = [NSPredicate predicateWithFormat:@"value ==[c] %@", @"a"];
  [self expectationForPredicate:hasText evaluatedWithObject:notes handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCUICoordinate *word = [[notes coordinateWithNormalizedOffset:CGVectorMake(0, 0)]
      coordinateWithOffset:CGVectorMake(15, 20)];
  XCUIElementQuery *items = [app descendantsMatchingType:XCUIElementTypeAny];
  XCUIElement *cut = app.menuItems[@"Cut"];
  [word pressForDuration:0.8];
  XCTAssertTrue([cut waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertTrue(keyboard.exists);
  [cut tap];
  [self expectationForPredicate:[NSCompoundPredicate notPredicateWithSubpredicate:hasText]
           evaluatedWithObject:notes handler:nil];
  [self waitForExpectationsWithTimeout:8 handler:nil];

  [word pressForDuration:0.8];
  XCUIElement *undo = app.menuItems[@"Undo"];
  XCTAssertTrue([undo waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [undo tap];
  [self expectationForPredicate:hasText evaluatedWithObject:notes handler:nil];
  [self waitForExpectationsWithTimeout:8 handler:nil];

  NSPredicate *menuDismissed = [NSPredicate predicateWithBlock:^BOOL(XCUIApplication *application, NSDictionary *bindings) {
    return application.menuItems.count == 0;
  }];
  [self expectationForPredicate:menuDismissed evaluatedWithObject:app handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCUIElement *lock = items[@"Read-only"];
  [[lock coordinateWithNormalizedOffset:CGVectorMake(0.12, 0.5)] pressForDuration:0.12];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"1"]
           evaluatedWithObject:lock handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [word pressForDuration:0.8];
  XCUIElement *copy = app.menuItems[@"Copy"];
  XCTAssertTrue([copy waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertFalse(cut.exists);
  XCTAssertFalse(app.menuItems[@"Paste"].exists);
  XCTAssertTrue([hasText evaluateWithObject:notes]);
}

- (void)selectFixtureFolder:(NSString *)name inApp:(XCUIApplication *)app {
  XCUIElement *root = app.cells[@"Inspection folders, Folder"].firstMatch;
  if ([root waitForExistenceWithTimeout:3]) {
    [[root coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.33)] tap];
  }
  XCUIElement *entry = [app.cells matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", name]].firstMatch;
  XCTAssertTrue([entry waitForExistenceWithTimeout:12], @"%@", app.debugDescription);
  [[entry coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.33)] tap];
  XCUIElement *reports = [app.cells matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", @"reports"]].firstMatch;
  XCTAssertTrue([reports waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCUIElement *open = app.buttons[@"Open"].firstMatch;
  XCTAssertTrue([open waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [open tap];
}

- (void)testDeclarativeFileDialogImportsCopiesOfSelectedFolders {
  self.continueAfterFailure = NO;
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--folder-dialog", @"-AppleLanguages", @"(en)"];
  [app launch];

  XCUIElement *one = app.buttons[@"Import one folder"];
  XCTAssertTrue([one waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [one tap];
  [self selectFixtureFolder:@"Pump notes" inApp:app];
  XCTAssertTrue([app.staticTexts[@"Imported 1 · Pump reading"] waitForExistenceWithTimeout:12],
                @"%@", app.debugDescription);

  XCUIElement *many = app.buttons[@"Import folders"];
  [many tap];
  [self selectFixtureFolder:@"Pump notes" inApp:app];
  XCUIElement *summary = app.alerts[@"Selected folders"];
  XCUIElement *add = summary.buttons[@"Add another folder"];
  XCTAssertTrue([add waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [add tap];
  [self selectFixtureFolder:@"Valve notes" inApp:app];
  XCUIElement *commit = summary.buttons[@"Import selected folders"];
  XCTAssertTrue([commit waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [commit tap];
  XCTAssertTrue([app.staticTexts[@"Imported 2 · Pump reading, Valve reading"] waitForExistenceWithTimeout:12],
                @"%@", app.debugDescription);

  [many tap];
  [self selectFixtureFolder:@"Pump notes" inApp:app];
  XCUIElement *cancel = summary.buttons[@"Cancel"];
  XCTAssertTrue([cancel waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [cancel tap];
  XCTAssertTrue([app.staticTexts[@"Folder selection canceled"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);
}

- (void)testRestoresSelectedNotesFolderAcrossLaunches {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *showNotes = app.buttons[@"Open field notes"];
  XCTAssertTrue([showNotes waitForExistenceWithTimeout:15]);
  [showNotes tap];
  XCUIElement *reload = app.buttons[@"Reload last folder"];
  XCTAssertFalse(reload.exists);
  XCUIElement *notes = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection notes"];
  [notes tap];
  XCUIElement *keyboard = [self visibleKeyboardInApp:app];
  [self dismissKeyboardIntroductionInApp:app];
  XCUIElement *letter = [keyboard.keys matchingPredicate:
      [NSPredicate predicateWithFormat:@"label ==[c] %@", @"a"]].firstMatch;
  [self tapKeyboardKey:letter inApp:app];
  NSPredicate *hasText = [NSPredicate predicateWithFormat:@"value ==[c] %@", @"a"];
  [self expectationForPredicate:hasText evaluatedWithObject:notes handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCUIElement *save = app.buttons[@"Save note to Files"];
  [[save coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  [self confirmPocketTasksExportDirectoryInApp:app];
  XCTAssertTrue([app.staticTexts[@"Saved to Files"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);

  XCUIElement *choose = app.buttons[@"Import notes from folder"];
  [[choose coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  [self confirmPocketTasksExportDirectoryInApp:app];
  XCUIElement *imported = app.staticTexts[@"1 imported from folder"];
  XCTAssertTrue([imported waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertTrue(reload.exists);

  [app terminate];
  [app launch];
  XCTAssertTrue([showNotes waitForExistenceWithTimeout:15]);
  [showNotes tap];
  XCTAssertTrue([reload waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertFalse([hasText evaluateWithObject:notes]);
  [[reload coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([imported waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertTrue([hasText evaluateWithObject:notes]);
}

- (void)testUIKitSerializesDeclarativeAndRustPanels {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--native-panels"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *start = app.buttons[@"Open panel queue"];
  XCTAssertTrue([start waitForExistenceWithTimeout:15]);
  [[start coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];

  XCUIElement *notice = app.alerts[@"System panel queue"];
  XCTAssertTrue([notice waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  [notice.buttons[@"OK"] tap];

  XCUIElement *open = [app.buttons matchingPredicate:
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"Open", @"Done"]]].firstMatch;
  XCTAssertTrue([open waitForExistenceWithTimeout:10], @"%@", app.debugDescription);
  XCUIElement *cancel = app.buttons[@"Cancel"];
  XCTAssertTrue([cancel waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [[cancel coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];

  XCUIElement *photo = [app.images matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH[c] %@", @"Photo,"]].firstMatch;
  XCTAssertTrue([photo waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCTAssertTrue(cancel.exists, @"%@", app.debugDescription);
  [[cancel coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];

  XCUIElement *activities = app.otherElements[@"ActivityListView"].firstMatch;
  XCTAssertTrue([activities waitForExistenceWithTimeout:12], @"%@", app.debugDescription);
  [[app coordinateWithNormalizedOffset:CGVectorMake(0.03, 0.08)] tap];

  XCTAssertTrue([app.staticTexts[@"Share canceled"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Notice completed"].exists);
  XCTAssertTrue(app.staticTexts[@"Files canceled"].exists);
  XCTAssertTrue(app.staticTexts[@"Photos canceled"].exists);
  XCTAssertTrue(start.enabled);
}

- (void)testUIKitAbortedPanelsDismissThenShowTheNextPanel {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--native-panels"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *start = app.buttons[@"Open timed panels"];
  XCTAssertTrue([start waitForExistenceWithTimeout:15]);
  [[start coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];

  XCUIElement *destination = [app.buttons matchingPredicate:
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"Open", @"Done"]]].firstMatch;
  XCTAssertTrue([destination waitForExistenceWithTimeout:6], @"%@", app.debugDescription);

  XCUIElement *activities = app.otherElements[@"ActivityListView"].firstMatch;
  XCTAssertTrue([activities waitForExistenceWithTimeout:11], @"%@", app.debugDescription);
  XCTAssertFalse(destination.exists);

  XCUIElement *notice = app.alerts[@"System panel queue"];
  XCTAssertTrue([notice waitForExistenceWithTimeout:11], @"%@", app.debugDescription);
  XCTAssertFalse(activities.exists);
  [notice.buttons[@"OK"] tap];
  XCTAssertTrue([app.staticTexts[@"Notice completed"] waitForExistenceWithTimeout:8],
                @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Files timed out"].exists);
  XCTAssertTrue(app.staticTexts[@"Share timed out"].exists);
  XCTAssertTrue(start.enabled);
}

- (void)testUIKitClosesDialogsWhenTheirInspectionPageIsRemoved {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--native-panels"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *pageNotice = app.alerts[@"Inspection page notice"];
  XCUIElement *pageFiles = app.otherElements[@"Browse View (Picker)"].firstMatch;
  XCUIElement *notice = app.alerts[@"System panel queue"];
  for (NSString *label in @[@"Open page notice", @"Open page Files"]) {
    XCUIElement *open = app.buttons[label];
    XCTAssertTrue([open waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
             evaluatedWithObject:open handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
    [[open coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
    XCUIElement *show = app.buttons[@"Show inspection dialog"];
    XCTAssertTrue([show waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
    [[show coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];

    XCUIElement *page = [label isEqualToString:@"Open page notice"] ? pageNotice : pageFiles;
    XCTAssertTrue([page waitForExistenceWithTimeout:6], @"%@", app.debugDescription);
    XCTAssertTrue([notice waitForExistenceWithTimeout:12], @"%@", app.debugDescription);
    XCTAssertFalse(page.exists);

    XCUIElement *ok = notice.buttons[@"OK"];
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
             evaluatedWithObject:ok handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
    [[ok coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
    XCTAssertTrue([app.staticTexts[@"Notice completed"] waitForExistenceWithTimeout:8],
                  @"%@", app.debugDescription);
    XCTAssertTrue(app.staticTexts[@"Inspection page closed"].exists);
    XCTAssertFalse(app.staticTexts[@"Inspection page received a reply"].exists);
    XCTAssertTrue(open.enabled);
  }
}

- (void)testUIKitColorPickerUsesTheInitialColorAndPreservesHiddenAlpha {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--native-colors"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *edit = app.buttons[@"Edit marker color"];
  XCTAssertTrue([edit waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCUIElement *initial = app.staticTexts[@"#2D638880"];
  XCTAssertTrue(initial.exists);

  XCUIElement *grid = app.buttons[@"Grid"].firstMatch;
  XCUIElement *close = [app.buttons matchingPredicate:
      [NSPredicate predicateWithFormat:@"label ==[c] %@", @"close"]].firstMatch;
  XCUIElement *opacity = app.textFields[@"Opacity"];
  [[edit coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([grid waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertEqualObjects(opacity.value, @"50%");
  XCTAssertTrue(close.exists, @"%@", app.debugDescription);
  [[close coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"enabled == YES AND hittable == YES"]
           evaluatedWithObject:edit handler:nil];
  [self waitForExpectationsWithTimeout:8 handler:nil];
  XCTAssertTrue(initial.exists);

  XCUIElement *alpha = app.switches[@"Edit opacity"];
  XCTAssertTrue(alpha.exists, @"%@", app.debugDescription);
  [[alpha coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"0"]
           evaluatedWithObject:alpha handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [[edit coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([grid waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCUIElement *white = app.otherElements[@"white 100"].firstMatch;
  XCTAssertTrue([white waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertFalse(opacity.exists);
  [[white coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([app.staticTexts[@"Preview #FFFFFF80"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertTrue(close.exists);
  [[close coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([app.staticTexts[@"#FFFFFF80"] waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCTAssertTrue(edit.enabled);

  [[alpha coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"1"]
           evaluatedWithObject:alpha handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [[edit coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([opacity waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
  XCUIElement *ramp = app.otherElements[@"Opacity"].firstMatch;
  XCTAssertTrue(ramp.exists, @"%@", app.debugDescription);
  XCUICoordinate *thumb = [ramp coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  XCUICoordinate *maximum = [ramp coordinateWithNormalizedOffset:CGVectorMake(1.03, 0.5)];
  [thumb pressForDuration:0.1 thenDragToCoordinate:maximum];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"100%"]
           evaluatedWithObject:opacity handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue([app.staticTexts[@"Preview #FFFFFFFF"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertTrue(close.exists);
  [[close coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] pressForDuration:0.12];
  XCTAssertTrue([app.staticTexts[@"#FFFFFFFF"] waitForExistenceWithTimeout:8], @"%@", app.debugDescription);
}

@end
