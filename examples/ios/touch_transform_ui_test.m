#import <XCTest/XCTest.h>

@interface XRackTouchTransformUITest : XCTestCase
@end

@implementation XRackTouchTransformUITest

- (void)testDirectPinchAndRotationStillReachTheGestureCanvas {
  self.continueAfterFailure = NO;
  XCUIApplication *app = [[XCUIApplication alloc] init];
  [app launch];

  XCUIElement *open = app.buttons[@"Open gestures"];
  XCTAssertTrue([open waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [open tap];
  XCUIElement *canvas = [app descendantsMatchingType:XCUIElementTypeAny][@"Gesture canvas"];
  XCTAssertTrue([canvas waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  [canvas pinchWithScale:1.6 velocity:1.0];
  XCTAssertTrue([app.staticTexts[@"Ends 1"] waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Starts 1"].exists);
  XCTAssertFalse(app.staticTexts[@"Zoom 100"].exists);

  [canvas rotate:M_PI / 5.0 withVelocity:1.0];
  XCTAssertTrue([app.staticTexts[@"Ends 2"] waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertTrue(app.staticTexts[@"Starts 2"].exists);
  XCTAssertFalse(app.staticTexts[@"Angle 0"].exists);
  XCTAssertTrue(app.staticTexts[@"Taps 0"].exists);

  [app.buttons[@"Tap pad"] tap];
  XCTAssertTrue([app.staticTexts[@"Taps 1"] waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
}

@end
