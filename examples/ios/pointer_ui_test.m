#import <UIKit/UIKit.h>
#import <XCTest/XCTest.h>

@interface PointerInspectionUITest : XCTestCase
@end

@implementation PointerInspectionUITest

- (void)testIPadPointerHoversAndScrollsWithoutReplacingTouch {
  XCTSkipIf(UIDevice.currentDevice.userInterfaceIdiom != UIUserInterfaceIdiomPad,
            @"Requires an iPad simulator with pointer input");
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--ipad-pointer"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];
  XCUIElement *sensor = app.buttons[@"Inspect sensor"];
  XCTAssertTrue([sensor waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCUIElement *log = [app descendantsMatchingType:XCUIElementTypeAny][@"Inspection log"];
  XCUICoordinate *logCenter = [log coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  XCTAssertTrue(app.staticTexts[@"Pointer outside log"].exists);
  // Move the simulated pointer from outside the log before testing hover.
  [[sensor coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] scrollByDeltaX:0 deltaY:-10];
  [logCenter scrollByDeltaX:0 deltaY:-48];
  XCTAssertTrue(app.staticTexts[@"Log at top"].exists);
  XCUIElement *wheelCount = [app.staticTexts matchingPredicate:
      [NSPredicate predicateWithFormat:@"label BEGINSWITH %@", @"Wheel events "]].firstMatch;
  XCTAssertTrue(wheelCount.exists, @"%@", app.debugDescription);
  NSString *beforeScroll = wheelCount.label;
  [logCenter scrollByDeltaX:0 deltaY:64];
  XCTAssertTrue([app.staticTexts[@"Log scrolled"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCUIElement *entered = [app.staticTexts matchingPredicate:
      [NSPredicate predicateWithFormat:@"label IN %@", @[@"Pointer over log", @"Pointer left log"]]].firstMatch;
  XCTAssertTrue([entered waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Inspections 0"].exists);
  XCTAssertNotEqualObjects(wheelCount.label, beforeScroll);
  NSString *previousCount = wheelCount.label;
  XCUICoordinate *start = [log coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.12)];
  XCUICoordinate *end = [log coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.90)];
  [start pressForDuration:0.08 thenDragToCoordinate:end withVelocity:XCUIGestureVelocitySlow
           thenHoldForDuration:0];
  XCTAssertTrue([app.staticTexts[@"Log at top"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
  XCTAssertEqualObjects(wheelCount.label, previousCount);
  [sensor tap];
  XCTAssertTrue([app.staticTexts[@"Inspections 1"] waitForExistenceWithTimeout:5],
                @"%@", app.debugDescription);
}

- (void)testIPadSecondaryClickOpensContextMenuWithoutActivatingControl {
  XCTSkipIf(UIDevice.currentDevice.userInterfaceIdiom != UIUserInterfaceIdiomPad,
            @"Requires an iPad simulator with pointer input");
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--ipad-pointer"];
  [XCUIDevice sharedDevice].orientation = UIDeviceOrientationPortrait;
  [app launch];

  XCUIElement *sensor = app.buttons[@"Inspect sensor"];
  XCTAssertTrue([sensor waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  XCUICoordinate *sensorCenter = [sensor coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)];
  XCUIElement *openLog = [app descendantsMatchingType:XCUIElementTypeAny][@"Open sensor log"];

  [sensorCenter rightClick];
  XCTAssertTrue([openLog waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Inspections 0"].exists);
  [openLog tap];
  XCTAssertTrue([app.staticTexts[@"Sensor log opens 1"] waitForExistenceWithTimeout:5]);

  [XCUIElement performWithKeyModifiers:XCUIKeyModifierControl block:^{
    [sensorCenter click];
  }];
  XCTAssertTrue([openLog waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Inspections 0"].exists);
  [openLog tap];
  XCTAssertTrue([app.staticTexts[@"Sensor log opens 2"] waitForExistenceWithTimeout:5]);

  [sensorCenter pressForDuration:0.7];
  XCTAssertTrue([openLog waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Inspections 0"].exists);
  [openLog tap];
  XCTAssertTrue([app.staticTexts[@"Sensor log opens 3"] waitForExistenceWithTimeout:5]);

  [sensorCenter click];
  XCTAssertTrue([app.staticTexts[@"Inspections 1"] waitForExistenceWithTimeout:5]);
  [sensor tap];
  XCTAssertTrue([app.staticTexts[@"Inspections 2"] waitForExistenceWithTimeout:5]);
}

@end
