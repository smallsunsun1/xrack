#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

@interface SampleTransformEvent : UIEvent
@property(nonatomic) UIEventType sampleType;
@end

@implementation SampleTransformEvent
- (UIEventType)type { return self.sampleType; }
@end

@protocol TrackpadTransformReceiver
- (void)pinchChanged:(UIPinchGestureRecognizer *)recognizer;
- (void)rotationChanged:(UIRotationGestureRecognizer *)recognizer;
@end

@interface PinchSample : UIPinchGestureRecognizer
@property(nonatomic) CGPoint samplePoint;
@property(nonatomic) UIGestureRecognizerState sampleState;
@property(nonatomic) CGFloat sampleScale;
@property(nonatomic, weak) UIView *sampleView;
@end

@implementation PinchSample
- (UIView *)view { return self.sampleView; }
- (UIGestureRecognizerState)state { return self.sampleState; }
- (CGFloat)scale { return self.sampleScale; }
- (CGPoint)locationInView:(UIView *)view { return self.samplePoint; }
@end

@interface RotationSample : UIRotationGestureRecognizer
@property(nonatomic) CGPoint samplePoint;
@property(nonatomic) UIGestureRecognizerState sampleState;
@property(nonatomic) CGFloat sampleRotation;
@property(nonatomic, weak) UIView *sampleView;
@end

@implementation RotationSample
- (UIView *)view { return self.sampleView; }
- (UIGestureRecognizerState)state { return self.sampleState; }
- (CGFloat)rotation { return self.sampleRotation; }
- (CGPoint)locationInView:(UIView *)view { return self.samplePoint; }
@end

static UIGestureRecognizer *FindRecognizer(UIView *view, Class type) {
  for (UIGestureRecognizer *recognizer in view.gestureRecognizers) {
    if ([recognizer isKindOfClass:type]) {
      return recognizer;
    }
  }
  for (UIView *child in view.subviews) {
    UIGestureRecognizer *recognizer = FindRecognizer(child, type);
    if (recognizer != nil) {
      return recognizer;
    }
  }
  return nil;
}

static BOOL HasLabels(UIView *host, NSArray<NSString *> *labels) {
  for (NSString *label in labels) {
    if (AccessibleElement(host, label) == nil) {
      return NO;
    }
  }
  return YES;
}

static BOOL WaitFor(BOOL (^condition)(void)) {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:ready object:nil];
  return [XCTWaiter waitForExpectations:@[expectation] timeout:8] == XCTWaiterResultCompleted;
}

@interface XRackTrackpadTransformTest : XCTestCase
@end

@implementation XRackTrackpadTransformTest

- (void)testNativeTransformRoutingCombinesPinchAndRotation {
  self.continueAfterFailure = NO;
  XCTAssertTrue(WaitFor(^BOOL {
    return AccessibleElement(WindowAccessibilityHost(), @"Open gestures") != nil;
  }));
  UIView *host = WindowAccessibilityHost();
  UIGestureRecognizer *pinch = FindRecognizer(host.window, UIPinchGestureRecognizer.class);
  UIGestureRecognizer *rotation = FindRecognizer(host.window, UIRotationGestureRecognizer.class);
  XCTAssertNotNil(pinch);
  XCTAssertNotNil(rotation);
  id<UIGestureRecognizerDelegate> delegate = pinch.delegate;
  XCTAssertNotNil(delegate);
  XCTAssertEqual(delegate, rotation.delegate);
  XCTAssertEqual(pinch.allowedTouchTypes.count, 0);
  XCTAssertEqual(rotation.allowedTouchTypes.count, 0);
  XCTAssertFalse(pinch.cancelsTouchesInView);
  XCTAssertFalse(rotation.cancelsTouchesInView);
  SampleTransformEvent *event = [[SampleTransformEvent alloc] init];
  event.sampleType = UIEventTypeTouches;
  XCTAssertFalse([delegate gestureRecognizer:pinch shouldReceiveEvent:event]);
  XCTAssertFalse([delegate gestureRecognizer:rotation shouldReceiveEvent:event]);
  event.sampleType = UIEventTypeTransform;
  XCTAssertTrue([delegate gestureRecognizer:pinch shouldReceiveEvent:event]);
  XCTAssertTrue([delegate gestureRecognizer:rotation shouldReceiveEvent:event]);
  XCTAssertTrue([delegate gestureRecognizer:pinch shouldRecognizeSimultaneouslyWithGestureRecognizer:rotation]);

  XCTAssertTrue([AccessibleElement(host, @"Open gestures") accessibilityActivate]);
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Tap pad", @"Gesture idle", @"Starts 0"]); }));
  CGRect frame = [AccessibleElement(host, @"Tap pad") accessibilityFrame];
  CGPoint point = CGPointMake(CGRectGetMidX(frame), CGRectGetMidY(frame));
  UIView *inputView = pinch.view;
  point = [inputView convertPoint:point fromCoordinateSpace:inputView.window.screen.coordinateSpace];

  // Feed recognized UIKit samples through the installed delegate's target actions.
  // XCTest's public pinch/rotate gestures generate touches rather than trackpad transforms.
  id<TrackpadTransformReceiver> receiver = (id<TrackpadTransformReceiver>)delegate;
  PinchSample *scale = [[PinchSample alloc] initWithTarget:nil action:nil];
  scale.sampleView = inputView;
  scale.samplePoint = point;
  scale.sampleState = UIGestureRecognizerStateBegan;
  scale.sampleScale = 1.25;
  [receiver pinchChanged:scale];

  RotationSample *angle = [[RotationSample alloc] initWithTarget:nil action:nil];
  angle.sampleView = rotation.view;
  angle.samplePoint = point;
  angle.sampleState = UIGestureRecognizerStateBegan;
  angle.sampleRotation = 0.0;
  [receiver rotationChanged:angle];

  scale.sampleState = UIGestureRecognizerStateChanged;
  scale.sampleScale = 1.6;
  [receiver pinchChanged:scale];
  angle.sampleState = UIGestureRecognizerStateChanged;
  angle.sampleRotation = M_PI / 6.0;
  [receiver rotationChanged:angle];
  XCTAssertTrue(WaitFor(^BOOL {
    return HasLabels(host, @[@"Gesture active", @"Starts 1", @"Zoom 160", @"Angle 30"]);
  }));

  scale.sampleState = UIGestureRecognizerStateEnded;
  scale.sampleScale = 1.8;
  [receiver pinchChanged:scale];
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Gesture active", @"Zoom 180", @"Ends 0"]); }));
  angle.sampleState = UIGestureRecognizerStateEnded;
  [receiver rotationChanged:angle];
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Gesture idle", @"Zoom 180", @"Angle 30", @"Ends 1"]); }));

  scale.sampleState = UIGestureRecognizerStateBegan;
  scale.sampleScale = 1.2;
  [receiver pinchChanged:scale];
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Gesture active", @"Zoom 216", @"Starts 2"]); }));
  scale.sampleState = UIGestureRecognizerStateCancelled;
  [receiver pinchChanged:scale];
  XCTAssertTrue(WaitFor(^BOOL {
    return HasLabels(host, @[@"Gesture idle", @"Zoom 180", @"Angle 30", @"Ends 1", @"Cancels 1", @"Taps 0"]);
  }));

  XCTAssertTrue([AccessibleElement(host, @"Tap pad") accessibilityActivate]);
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Taps 1"]); }));

  scale.sampleState = UIGestureRecognizerStateBegan;
  scale.sampleScale = 1.5;
  [receiver pinchChanged:scale];
  XCTAssertTrue(WaitFor(^BOOL { return HasLabels(host, @[@"Gesture active", @"Zoom 270", @"Starts 3"]); }));
  [NSNotificationCenter.defaultCenter postNotificationName:UIApplicationWillResignActiveNotification
                                                    object:UIApplication.sharedApplication];
  XCTAssertTrue(WaitFor(^BOOL {
    return HasLabels(host, @[@"Gesture idle", @"Zoom 180", @"Cancels 2"]);
  }));
  [NSNotificationCenter.defaultCenter postNotificationName:UIApplicationDidBecomeActiveNotification
                                                    object:UIApplication.sharedApplication];
}

@end
