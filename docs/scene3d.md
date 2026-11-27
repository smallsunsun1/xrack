# 3D 场景

`SceneView3D` 把 wgpu 绘制的场景放进普通 XRack 窗口。视口和按钮、表格、工具栏一样参与 2D 布局。用 `.xr` 绑定模型、相机和材质，应用逻辑可以通过属性和回调更新场景。

## 跑起第一个场景

在仓库根目录运行下方命令。示例同时展示矩形、圆柱和圆锥，以及可以切换材质和相机的 2D 控件。

```sh
bazel run //examples/puzzle3d:play_primitive_meshes
```

在 VS Code 中也可以打开 [示例源码](../examples/puzzle3d/primitive_meshes.xr)，运行 **XRack: Open Live Preview**。预览工具栏的 **3D Stats** 显示视口尺寸、网格、阴影、粒子和通道耗时。GPU 耗时依赖设备支持时间戳查询。 [VS Code 与预览](editor.md)

## 用 .xr 搭建视口

下面这个组件把一个立方体和一个旋转按钮放进同一列。按钮改变角度时，`model <- …` 会重新计算场景记录。记录名 `Actor` 可以替换，编译器按字段及其类型检查场景接口。

```xr
struct Actor {
    id: string, mesh: string,
    x: float, y: float, z: float,
    scale-x: float, scale-y: float, scale-z: float,
    yaw: float, pitch: float, roll: float, color: color,
}

export component BoxDemo inherits Window {
    width: 440px;
    height: 320px;
    private property <float> angle: 20;

    VerticalLayout {
        view := SceneView3D {
            vertical-stretch: 1;
            camera-yaw: 24;
            camera-pitch: 15;
            camera-distance: 4;
            scene-background: #202634;
            model <- [Actor {
                id: "box", mesh: "primitive:box",
                x: 0, y: 0, z: 0, scale-x: 1, scale-y: 1, scale-z: 1,
                yaw: root.angle, pitch: 0, roll: 0, color: #79c4ad,
            }];
        }
        turn := Button {
            text: "旋转";
            clicked => { root.angle += 15; }
        }
    }
}
```

- **模型与资源**。用 `@mesh-url("models/example.glb")` 引用随应用构建的本地模型。在 Bazel 里通过 `xr_ui_library(assets = [...])` 声明资源。需要打开用户选择的本地文件时，可用 `MeshLoader3D` 读取 `asset: meshasset3d`，再在记录字面量中写 `mesh-asset: loader.asset`。
- **相机与交互**。`view.project(world)` 返回投影结果，`view.pick(point)` 返回物体 ID，`view.hit(point)` 返回命中详情。拾取点相对于视口左上角，单位为逻辑像素。从世界坐标发起的物理查询使用 `Physics3D`，根据可见几何查询可以用视口上的 `ray-hit`。
- **灯光与材质**。视口接收灯光列表、环境与阴影设置。模型记录接收 PBR 材质和材质槽覆盖，贴图通过 `@texture-url` 引用。可用 `@material-shader-url` 加载可编程材质，用 `@shader-url` 加载视口后处理。
- **2D 与 3D 合成**。窗口直属的视口可以用 `render-mode: "underlay"` 或 `"overlay"` 选择相对普通内容的绘制层。也能把视口输出作为其他元素的纹理。
- **物理与粒子**。`Physics3D` 提供刚体世界、关节和空间查询。`particle-emitters` 和 `particle-affectors` 分别描述发射器与影响器，可以给粒子设置拖尾、模型和光照。

## 按功能找示例

下表中的目标可用 `bazel run //examples/puzzle3d:<目标>` 启动。在 VS Code 中也可以直接打开对应的 `.xr` 学习声明和绑定。

| 功能 | 示例源码 | Bazel 目标 |
| --- | --- | --- |
| 基础图元与拾取 | [primitive_meshes.xr](../examples/puzzle3d/primitive_meshes.xr) | `play_primitive_meshes` |
| 本地模型加载与点击拾取 | [declarative_mesh.xr](../examples/puzzle3d/declarative_mesh.xr) | `play_declarative_mesh` |
| 2D / 3D 叠层 | [window_layers.xr](../examples/puzzle3d/window_layers.xr) | `play_window_layers` |
| 视口输出作为纹理 | [viewport_texture.xr](../examples/puzzle3d/viewport_texture.xr) | `play_viewport_texture` |
| 自定义 WGSL 材质 | [material_shader.xr](../examples/puzzle3d/material_shader.xr) | `play_material_shader` |
| 命名渲染捕获 | [scene_capture.xr](../examples/puzzle3d/scene_capture.xr) | `play_scene_capture` |
| 光照贴图 | [lightmaps.xr](../examples/puzzle3d/lightmaps.xr) | `play_lightmaps` |
| 刚体关节 | [physics_joints.xr](../examples/puzzle3d/physics_joints.xr) | `play_physics_joints` |
| 粒子拖尾 | [particle_trails.xr](../examples/puzzle3d/particle_trails.xr) | `play_particle_trails` |

[完整视口属性与材质参考](controls.md#sceneview3d) · [视口查询方法用法](controls.md#直接查询-sceneview3d) · [全部 3D 示例](../examples/puzzle3d/)
