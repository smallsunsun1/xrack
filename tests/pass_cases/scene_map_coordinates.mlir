// RAW: "xr.geometry_function"{{.*}}value = "map-position-3d"
// RAW: "xr.geometry_function"{{.*}}value = "map-direction-3d"
// CHECK: "xr.geometry_function"{{.*}}value = "map-position-3d"{{.*}}!xr.point3d
// CHECK: "xr.geometry_function"{{.*}}value = "map-direction-3d"{{.*}}!xr.vector3d
// CHECK-NOT: "xr.geometry_function"
// CHECK: %[[POINT:[0-9]+]] = "xr.literal"() <{value = "5 6 7"}> : () -> !xr.point3d
// CHECK: "xr.property"(%[[POINT]]) <{name = "identity", visibility = "out"}>
// CHECK-NOT: "xr.geometry_function"
// CHECK: %[[DIRECTION:[0-9]+]] = "xr.literal"() <{value = "9 8 7"}> : () -> !xr.vector3d
// CHECK: "xr.property"(%[[DIRECTION]]) <{name = "missing", visibility = "out"}>
// CHECK-NOT: "xr.geometry_function"

module {
  "xr.record_decl"() <{fields = "[{\22name\22:\22id\22,\22ty\22:\22string\22},{\22name\22:\22mesh\22,\22ty\22:\22string\22},{\22name\22:\22x\22,\22ty\22:\22float\22},{\22name\22:\22y\22,\22ty\22:\22float\22},{\22name\22:\22z\22,\22ty\22:\22float\22},{\22name\22:\22scale-x\22,\22ty\22:\22float\22},{\22name\22:\22scale-y\22,\22ty\22:\22float\22},{\22name\22:\22scale-z\22,\22ty\22:\22float\22},{\22name\22:\22yaw\22,\22ty\22:\22float\22},{\22name\22:\22pitch\22,\22ty\22:\22float\22},{\22name\22:\22roll\22,\22ty\22:\22float\22},{\22name\22:\22color\22,\22ty\22:\22color\22}]", sym_name = "Actor"}> : () -> ()
  "xr.component"() <{exported = "true", sym_name = "CoordinatePlan"}> ({
    %empty = "xr.record_array"() <{value = ""}> : () -> !xr.recordlist<"Actor">
    "xr.property"(%empty) <{name = "nodes", visibility = "in"}> : (!xr.recordlist<"Actor">) -> ()
    %nodes = "xr.read"() <{owner = "root", value = "nodes"}> : () -> !xr.recordlist<"Actor">
    %world = "xr.literal"() <{value = "\22\22"}> : () -> !xr.string
    %rig = "xr.literal"() <{value = "\22rig\22"}> : () -> !xr.string
    %origin = "xr.literal"() <{value = "0 0 0"}> : () -> !xr.point3d
    %forward = "xr.literal"() <{value = "1 0 2"}> : () -> !xr.vector3d
    %local = "xr.geometry_function"(%nodes, %world, %rig, %origin, %origin) <{value = "map-position-3d"}> : (!xr.recordlist<"Actor">, !xr.string, !xr.string, !xr.point3d, !xr.point3d) -> !xr.point3d
    "xr.property"(%local) <{name = "local", visibility = "out"}> : (!xr.point3d) -> ()
    %heading = "xr.geometry_function"(%nodes, %rig, %world, %forward, %forward) <{value = "map-direction-3d"}> : (!xr.recordlist<"Actor">, !xr.string, !xr.string, !xr.vector3d, !xr.vector3d) -> !xr.vector3d
    "xr.property"(%heading) <{name = "heading", visibility = "out"}> : (!xr.vector3d) -> ()
    %point = "xr.literal"() <{value = "5 6 7"}> : () -> !xr.point3d
    %identity = "xr.geometry_function"(%empty, %world, %world, %point, %origin) <{value = "map-position-3d"}> : (!xr.recordlist<"Actor">, !xr.string, !xr.string, !xr.point3d, !xr.point3d) -> !xr.point3d
    "xr.property"(%identity) <{name = "identity", visibility = "out"}> : (!xr.point3d) -> ()
    %fallback = "xr.literal"() <{value = "9 8 7"}> : () -> !xr.vector3d
    %missing = "xr.geometry_function"(%empty, %rig, %world, %forward, %fallback) <{value = "map-direction-3d"}> : (!xr.recordlist<"Actor">, !xr.string, !xr.string, !xr.vector3d, !xr.vector3d) -> !xr.vector3d
    "xr.property"(%missing) <{name = "missing", visibility = "out"}> : (!xr.vector3d) -> ()
    "xr.element"() <{id = "root", kind = "Window"}> ({
      "xr.end"() : () -> ()
    }) : () -> ()
    "xr.end"() : () -> ()
  }) : () -> ()
}
