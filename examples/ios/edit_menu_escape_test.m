#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

@interface EditMenuHoldSample : UILongPressGestureRecognizer
@property(nonatomic, weak) UIView *sourceView;
@property(nonatomic) CGPoint point;
@property(nonatomic) UIGestureRecognizerState phase;
@end

@implementation EditMenuHoldSample
- (UIView *)view { return self.sourceView; }
- (CGPoint)locationInView:(UIView *)view { return self.point; }
- (UIGestureRecognizerState)state { return self.phase; }
@end

@protocol EditMenuHoldReceiver
- (void)xrackSelectedTextHeld:(UILongPressGestureRecognizer *)recognizer;
@end

static UIEditMenuInteraction *TextEditMenu(UIView *view) {
  for (id<UIInteraction> interaction in view.interactions) {
    if ([interaction isKindOfClass:UIEditMenuInteraction.class] &&
        [((UIEditMenuInteraction *)interaction).delegate
          isKindOfClass:NSClassFromString(@"XRackEditMenuDelegate")]) {
      return (UIEditMenuInteraction *)interaction;
    }
  }
  return nil;
}

@interface XRackEditMenuEscapeTest : XCTestCase
@end

@implementation XRackEditMenuEscapeTest

- (void)waitForLabel:(NSString *)label {
  NSPredicate *visible = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return AccessibleElement(WindowAccessibilityHost(), label) != nil;
  }];
  [self expectationForPredicate:visible evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
}

- (void)testUIKitEscapeDismissesTheEditMenuBeforeItsDialog {
  self.continueAfterFailure = NO;
  [self waitForLabel:@"Select draft"];
  UIView *host = WindowAccessibilityHost();
  XCTAssertTrue([AccessibleElement(host, @"Select draft") accessibilityActivate]);
  [self waitForLabel:@"Selected Survey"];
  id editor = AccessibleElement(host, @"Draft text");
  UIView *view = host.superview;
  id<EditMenuHoldReceiver> receiver = nil;
  for (UIGestureRecognizer *recognizer in view.gestureRecognizers) {
    if ([recognizer.delegate respondsToSelector:@selector(xrackSelectedTextHeld:)]) {
      receiver = (id<EditMenuHoldReceiver>)recognizer.delegate;
      break;
    }
  }
  XCTAssertNotNil(receiver);
  CGRect bounds = [editor accessibilityFrame];
  EditMenuHoldSample *hold = [[EditMenuHoldSample alloc] initWithTarget:nil action:nil];
  hold.sourceView = view;
  hold.point = [view convertPoint:CGPointMake(CGRectGetMinX(bounds) + 22, CGRectGetMidY(bounds))
                 fromCoordinateSpace:host.window.screen.coordinateSpace];
  hold.phase = UIGestureRecognizerStateBegan;
  [receiver xrackSelectedTextHeld:hold];
  hold.phase = UIGestureRecognizerStateEnded;
  [receiver xrackSelectedTextHeld:hold];
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIView *parent, NSDictionary *bindings) {
    UIEditMenuInteraction *menu = TextEditMenu(parent);
    return menu != nil && [menu locationInView:parent].x > 0;
  }] evaluatedWithObject:view handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCTAssertTrue([AccessibleElement(host, @"Move draft") accessibilityActivate]);
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(id field, NSDictionary *bindings) {
    return CGRectGetMinX([field accessibilityFrame]) > CGRectGetMinX(bounds) + 30;
  }] evaluatedWithObject:editor handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];

  XCTAssertTrue([host accessibilityPerformEscape]);
  // Accessibility actions are posted to XRack's event loop.
  XCTestExpectation *closedDialog = [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIView *root, NSDictionary *bindings) {
    return AccessibleElement(root, @"Draft text") == nil;
  }] evaluatedWithObject:host handler:nil];
  closedDialog.inverted = YES;
  [self waitForExpectationsWithTimeout:0.6 handler:nil];
  XCTAssertEqualObjects([editor accessibilityValue], @"Survey");
  XCTAssertNotNil(AccessibleElement(host, @"Selected Survey"));

  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Draft closed"];
  XCTAssertNil(AccessibleElement(host, @"Draft text"));
}

@end
