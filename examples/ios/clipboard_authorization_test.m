#import <XCTest/XCTest.h>

@interface XRackClipboardAuthorizationTest : XCTestCase
@end

@implementation XRackClipboardAuthorizationTest

- (void)testCrossAppPasteKeepsItsOriginalSelectionAcrossAuthorization {
  self.continueAfterFailure = NO;
  NSString *decision = NSProcessInfo.processInfo.environment[@"XRACK_PASTE_TEST_DECISION"];
  XCTAssertTrue([decision isEqualToString:@"allow"] || [decision isEqualToString:@"deny"]);
  BOOL allow = [decision isEqualToString:@"allow"];

  XCUIApplication *source = [[XCUIApplication alloc]
      initWithBundleIdentifier:@"dev.xrack.clipboard-fixture"];
  [source launch];
  XCUIElement *copy = source.buttons[@"Copy fixture text"];
  XCTAssertTrue([copy waitForExistenceWithTimeout:10], @"%@", source.debugDescription);
  [copy tap];
  XCTAssertTrue([source.buttons[@"Fixture text copied"] waitForExistenceWithTimeout:5]);

  XCUIApplication *app = [[XCUIApplication alloc] init];
  app.launchArguments = @[@"--clipboard-authorization"];
  [app launch];
  XCUIElement *destination = [app descendantsMatchingType:XCUIElementTypeAny][@"Paste destination"];
  XCTAssertTrue([destination waitForExistenceWithTimeout:15], @"%@", app.debugDescription);
  [app.buttons[@"Select emoji"] tap];
  XCUIElement *selection = app.staticTexts[@"Selected 🧪"];
  XCTAssertTrue([selection waitForExistenceWithTimeout:5], @"%@", app.debugDescription);
  XCTAssertEqualObjects(destination.value, @"A🧪B");

  XCUIApplication *springboard = [[XCUIApplication alloc]
      initWithBundleIdentifier:@"com.apple.springboard"];
  NSPredicate *origin = [NSPredicate predicateWithFormat:@"label CONTAINS %@ AND label CONTAINS %@",
      @"XRack Controls", @"Clipboard Fixture"];
  XCUIElement *alert = [springboard.alerts matchingPredicate:origin].firstMatch;
  XCTAssertFalse(alert.exists);
  [app.buttons[@"Paste into selection"] tap];

  // UIKit's pasteboard read waits for the system response. Query SpringBoard,
  // because a snapshot of the app can block until the response has been tapped.
  NSArray<NSString *> *labels = allow
      ? @[@"Allow Paste", @"允许粘贴"]
      : @[@"Don’t Allow Paste", @"Don't Allow Paste", @"不允许粘贴"];
  NSPredicate *choice = [NSPredicate predicateWithFormat:@"label IN %@", labels];
  XCUIElement *response = [alert.buttons matchingPredicate:choice].firstMatch;
  XCTAssertTrue([response waitForExistenceWithTimeout:10],
      @"Expected system paste authorization. SpringBoard: %@", springboard.debugDescription);
  CGRect frame = response.frame;
  XCTAssertTrue(frame.size.width > 0 && frame.size.height > 0);
  [[response coordinateWithNormalizedOffset:CGVectorMake(0.5, 0.5)] tap];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"exists == false"]
           evaluatedWithObject:alert handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];

  NSString *expected = allow ? @"Aclipboard sampleB" : @"A🧪B";
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", expected]
           evaluatedWithObject:destination handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];

  if (allow) {
    XCTAssertTrue(app.staticTexts[@"Edits 1"].exists, @"%@", app.debugDescription);
    [app.buttons[@"Undo edit"] tap];
    [self expectationForPredicate:[NSPredicate predicateWithFormat:@"value == %@", @"A🧪B"]
             evaluatedWithObject:destination handler:nil];
    [self waitForExpectationsWithTimeout:5 handler:nil];
  } else {
    XCTAssertTrue(selection.exists, @"%@", app.debugDescription);
    XCTAssertTrue(app.staticTexts[@"Edits 0"].exists);
    XCTAssertFalse(app.buttons[@"Undo edit"].enabled);
  }
}

@end
