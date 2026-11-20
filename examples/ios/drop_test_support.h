#pragma once

#include "accessibility_test_support.h"

@interface XRackDropTestSession : NSObject
@property(nonatomic, copy) NSArray<UIDragItem *> *items;
@property(nonatomic) CGPoint screenPoint;
@end

@implementation XRackDropTestSession
- (CGPoint)locationInView:(UIView *)view {
  return [view.window.screen.coordinateSpace convertPoint:self.screenPoint toCoordinateSpace:view];
}
@end

static UIDropInteraction *DropInteraction(UIView *host) {
  for (id<UIInteraction> interaction in host.superview.interactions) {
    if ([interaction isKindOfClass:UIDropInteraction.class] &&
        [((UIDropInteraction *)interaction).delegate
          isKindOfClass:NSClassFromString(@"XRackIOSFileDropDelegate")]) {
      return (UIDropInteraction *)interaction;
    }
  }
  return nil;
}

static id<UIDropSession> Session(NSArray<NSItemProvider *> *providers, CGPoint point) {
  NSMutableArray<UIDragItem *> *items = [NSMutableArray arrayWithCapacity:providers.count];
  for (NSItemProvider *provider in providers) {
    [items addObject:[[UIDragItem alloc] initWithItemProvider:provider]];
  }
  XRackDropTestSession *session = [[XRackDropTestSession alloc] init];
  session.items = items;
  session.screenPoint = point;
  // The native delegate reads only items and location from the session.
  return (id<UIDropSession>)session;
}
