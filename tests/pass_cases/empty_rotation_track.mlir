// RAW: "xr.geometry_function"{{.*}}value = "quaternion-track"
// RAW: "xr.geometry_function"{{.*}}value = "quaternion-sample"
// CHECK-NOT: value = "quaternion-track"
// CHECK-NOT: value = "quaternion-sample"
// CHECK: %[[IDENTITY:[0-9]+]] = "xr.literal"() <{value = "0 0 0 1"}> : () -> !xr.quaternion3d
// CHECK: "xr.property"(%[[IDENTITY]]) <{name = "pose", visibility = "out"}>

module {
  "xr.record_decl"() <{fields = "[{\22name\22:\22time\22,\22ty\22:\22float\22},{\22name\22:\22orientation\22,\22ty\22:\22quaternion3d\22}]", sym_name = "RotationKey"}> : () -> ()
  "xr.component"() <{exported = "true", sym_name = "EmptyTrack"}> ({
    %0 = "xr.record_array"() <{value = ""}> : () -> !xr.recordlist<"RotationKey">
    %1 = "xr.geometry_function"(%0) <{value = "quaternion-track"}> : (!xr.recordlist<"RotationKey">) -> !xr.rotationtrack3d
    %2 = "xr.literal"() <{value = "1"}> : () -> !xr.float
    %3 = "xr.geometry_function"(%1, %2) <{value = "quaternion-sample"}> : (!xr.rotationtrack3d, !xr.float) -> !xr.quaternion3d
    "xr.property"(%3) <{name = "pose", visibility = "out"}> : (!xr.quaternion3d) -> ()
    "xr.element"() <{id = "", kind = "Window"}> ({
      "xr.end"() : () -> ()
    }) : () -> ()
    "xr.end"() : () -> ()
  }) : () -> ()
}
