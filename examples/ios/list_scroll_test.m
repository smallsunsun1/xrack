#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

@interface XRackListScrollTest : XCTestCase
@end

@implementation XRackListScrollTest

- (void)waitUntilVisible:(NSString *)label {
  NSPredicate *visible = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return AccessibleElement(WindowAccessibilityHost(), label) != nil;
  }];
  [self expectationForPredicate:visible evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
}

- (void)testUIKitPagesVirtualRowsAndLeavesTheirSelectionAlone {
  self.continueAfterFailure = NO;
  [self waitUntilVisible:@"Pump pressure"];
  UIView *host = WindowAccessibilityHost();
  id pump = AccessibleElement(host, @"Pump pressure");
  NSString *selection = @"Moves: 0 · Selected row: 1";
  XCTAssertNotNil(AccessibleElement(host, selection));
  XCTAssertNil(AccessibleElement(host, @"Cooling fan"));
  XCTAssertFalse([pump accessibilityScroll:UIAccessibilityScrollDirectionDown]);
  XCTAssertTrue([pump accessibilityScroll:UIAccessibilityScrollDirectionUp]);

  [self waitUntilVisible:@"Cooling fan"];
  XCTAssertNil(AccessibleElement(host, @"Pump pressure"));
  XCTAssertNotNil(AccessibleElement(host, selection));
  XCTAssertFalse([pump accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  id fan = AccessibleElement(host, @"Cooling fan");
  XCTAssertFalse([fan accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertTrue([fan accessibilityScroll:UIAccessibilityScrollDirectionDown]);

  [self waitUntilVisible:@"Pump pressure"];
  XCTAssertNil(AccessibleElement(host, @"Cooling fan"));
  XCTAssertNotNil(AccessibleElement(host, selection));
}

@end
