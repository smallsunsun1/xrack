#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

@interface WheelEventSample : UIEvent
@property(nonatomic) UIKeyModifierFlags flags;
@end

@implementation WheelEventSample
- (UIEventType)type { return UIEventTypeScroll; }
- (UIKeyModifierFlags)modifierFlags { return self.flags; }
@end

@interface WheelPanSample : UIPanGestureRecognizer
@property(nonatomic, weak) UIView *sampleView;
@property(nonatomic) UIGestureRecognizerState sampleState;
@property(nonatomic) CGPoint point;
@property(nonatomic) CGPoint translation;
@end

@implementation WheelPanSample
- (UIView *)view { return self.sampleView; }
- (UIGestureRecognizerState)state { return self.sampleState; }
- (CGPoint)locationInView:(UIView *)view { return self.point; }
- (CGPoint)translationInView:(UIView *)view { return self.translation; }
- (UIKeyModifierFlags)modifierFlags { return 0; }
@end

@protocol WheelReceiver <UIGestureRecognizerDelegate>
- (void)scrollChanged:(UIPanGestureRecognizer *)recognizer;
@end

static UIPanGestureRecognizer *WheelRecognizer(UIView *view) {
  for (UIGestureRecognizer *recognizer in view.gestureRecognizers) {
    if ([recognizer isKindOfClass:UIPanGestureRecognizer.class] &&
        [recognizer.delegate respondsToSelector:@selector(scrollChanged:)]) {
      return (UIPanGestureRecognizer *)recognizer;
    }
  }
  for (UIView *child in view.subviews) {
    UIPanGestureRecognizer *recognizer = WheelRecognizer(child);
    if (recognizer != nil) {
      return recognizer;
    }
  }
  return nil;
}

@interface XRackPointerScrollTest : XCTestCase
@end

@implementation XRackPointerScrollTest

- (void)waitForLabel:(NSString *)label {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return AccessibleElement(WindowAccessibilityHost(), label) != nil;
  }];
  XCTNSPredicateExpectation *expectation = [[XCTNSPredicateExpectation alloc] initWithPredicate:ready
                                                                                          object:nil];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:10], XCTWaiterResultCompleted,
                 @"Expected %@", label);
}

- (void)testModifiersFromEachUIKitScrollEvent {
  self.continueAfterFailure = NO;
  [self waitForLabel:@"Inspection log"];
  UIView *host = WindowAccessibilityHost();
  UIPanGestureRecognizer *pan = WheelRecognizer(host.window);
  XCTAssertNotNil(pan);
  id<WheelReceiver> receiver = (id<WheelReceiver>)pan.delegate;
  CGRect frame = [AccessibleElement(host, @"Inspection log") accessibilityFrame];
  WheelPanSample *sample = [[WheelPanSample alloc] initWithTarget:nil action:nil];
  sample.sampleView = pan.view;
  sample.point = [pan.view convertPoint:CGPointMake(CGRectGetMidX(frame), CGRectGetMidY(frame))
                   fromCoordinateSpace:host.window.screen.coordinateSpace];
  sample.translation = CGPointMake(0, -64);
  sample.sampleState = UIGestureRecognizerStateBegan;

  WheelEventSample *event = [[WheelEventSample alloc] init];
  event.flags = UIKeyModifierCommand;
  XCTAssertTrue([receiver gestureRecognizer:pan shouldReceiveEvent:event]);
  [receiver scrollChanged:sample];
  [self waitForLabel:@"Log text enlarged"];
  XCTAssertNotNil(AccessibleElement(host, @"Log at top"));

  sample.sampleState = UIGestureRecognizerStateEnded;
  [receiver scrollChanged:sample];
  event.flags = 0;
  sample.sampleState = UIGestureRecognizerStateBegan;
  XCTAssertTrue([receiver gestureRecognizer:pan shouldReceiveEvent:event]);
  [receiver scrollChanged:sample];
  [self waitForLabel:@"Log scrolled"];
  XCTAssertNotNil(AccessibleElement(host, @"Log text enlarged"));
}

@end
