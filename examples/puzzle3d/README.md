# 3D 解谜房间

在 macOS 上打开 `puzzle.xr`，运行 **XRack: Open Live Preview**。在场景内拖动鼠标绕视，滚轮缩放，点击物体选中。侧栏显示物体 ID 和命中距离，画面上的小圆点标出最近一次点击的世界位置。开启「穿透选择」后点击场景，再按「下一个命中」可选中遮挡物后方的物体。「俯视地图」切换到正交相机，画面里的物体仍可点击，「出口」标签通过同一相机投影跟随世界坐标。改变 `light-yaw` 或 `panel-width` 可立即看到光照或布局变化。

在 VS Code 的 **Tasks: Run Task** 中选择 **XRack: Play 3D Puzzle**，或者从仓库根目录运行：

```sh
bazel run --repo_env=CARGO_BAZEL_ISOLATED=false --run_env=XRACK_LIVE_PROJECT="$PWD" //examples/puzzle3d:play_live
```

方向键、WASD 和侧栏按钮可以移动。靠近琥珀、蓝色球形机关后依次点击，或在选中以后按 E，让门绕 `gate-hinge` 父节点转开，再走到出口。侧栏也有可直接聚焦的机关按钮。

在 VS Code 执行 **XRack: Sync Editor to Running App** 后编辑灯光或侧栏尺寸。窗口内的场景会同步，人物位置和机关状态仍由 Rust 宿主持有。无效修改保留上一个可运行场景。

房间使用 `SceneView3D.lights` 绑定一盏方向光和一盏暖色点光。方向光的角度由 `light-yaw` 控制，暖色点光挂在玩家的 `player-rig` 上，随玩家移动。触发琥珀机关会提高点光的强度。方向光和点光为房间、人物和机关投射阴影。方向光使用三个级联覆盖从相机起 24 个世界单位。绕视时复用暖色点光的深度图，移动或打开门时更新相关阴影。房间还使用 `models/studio.hdr` 提供低强度的环境照明，程序生成的 HDR 探针随示例分发。

`//examples/puzzle3d:play` 使用同一份 `.xr` 的 AOT 产物。房间用 `[SceneObject3D]` 记录模型描述，包含物体 ID、网格资源键、变换和颜色。琥珀机关的 `models/amber.glb` 包含 JPEG 底色和 PNG 法线贴图，切线在导入时生成。出口标记来自 `models/exit.gltf`、`models/exit.bin` 和带 alpha 裁剪的 `models/exit.png`。门后的彩色玻璃 `models/glass.gltf` 有两层透明子网格。`SceneView3D.model` 和 `Scene3D.pick` 使用同一个模型，渲染器复用 GPU 网格和纹理，再将离屏结果与普通 2D 控件合成。

对公开的记录列表属性，AOT 宿主可以用生成的 `bind_model_*` 接入可变模型，解释器宿主可以用 `Instance::set` 绑定 `RecordList`。更新坐标、网格资源键或颜色会改变后续绘制和拾取。原生热重载示例在 `.xr` 内绑定 `room`，Rust 宿主通过玩家位置和机关状态驱动它。Rust 也可以构造 `SceneObject`，用 `Ray3D::new` 和 `raycast` 查询网格的表面交点。

## glTF 节点动画

运行 `bazel run //examples/puzzle3d:play_animation` 打开动画示例，也可以在 VS Code 中预览 [animation.xr](animation.xr)。计时器驱动 `animation-time`，窗口可以暂停，并在形变、骨骼蒙皮、节点平移和节点旋转片段之间切换。默认片段用两个面板演示单独的形变和形变叠加蒙皮。拖动「形变」滑块可以同时控制两个网格的 `extend` 目标，其他目标继续播放片段。暂停后可用滑块调整形变，也可以在 VS Code 中修改 `time` 的初值来查看指定的动画姿态。


## glTF 动画过渡

运行 `bazel run //examples/puzzle3d:play_animation_blend`，或在 VS Code 预览 [animation_blend.xr](animation_blend.xr)。上排滑块混合两个片段，下排滑块分别调节它们的时间。从左到右可以观察节点平移与旋转、骨骼动画过渡到初始姿态，以及两个形变片段之间的过渡。

## 网格细节 LOD

运行 `bazel run //examples/puzzle3d:play_lod`，或在 VS Code 预览 [lod.xr](lod.xr)。拖动相机距离滑块，蓝色物体会依次使用球体、八面体和盒体。切到正交相机后，相机距离变化时物体的投影尺寸和细节级别保持一致。「始终用主网格」用于对照同一视角的画面。

示例中的「自动减面」会换到带两种材质的密集 glTF 球体，并使用 `@mesh-url(..., auto-lod)` 生成的级别。「始终用主网格」可在同一相机位置做画面对照。

开启「蒙皮与形变」后，拖动姿态滑块可以查看带两个关节和两个形变目标的网格。相机距离与「始终用主网格」可用于对照同一动画时刻的不同细节级别。

## 视口画质

运行 `bazel run //examples/puzzle3d:play_quality` 或在 VS Code 中预览 [quality.xr](quality.xr)。切换「MSAA」或「SSAA」观察盒体斜边。启用 SSAA 时，视口以 1.5 倍长宽渲染再缩小。开启「距离雾」对照两个盒子的不同深度。再开启「HDR」和「玻璃」，可以对照色调映射和透明混合参与时的效果。

## 多视口内容层

运行 `bazel run //examples/puzzle3d:play_content_layers`，或在 VS Code 预览 [content_layers.xr](content_layers.xr)。两个视口使用同一场景。左侧可切换主层、目标层、全部层或清空，右侧通过具名相机查看第二层。点击左侧模型可观察拾取结果。

## 视口纹理尺寸

运行 `bazel run //examples/puzzle3d:play_texture_size`，或在 VS Code 预览 [texture_size.xr](texture_size.xr)。按钮切换自动、半分辨率和 192×96 固定纹理。右侧滑块改变逻辑视口宽度。

## 3D 视口纹理缓存

运行 `bazel run //examples/puzzle3d:play_viewport_cache`，或在 VS Code 预览 [viewport_cache.xr](viewport_cache.xr)。点「自动计数」持续刷新 2D 控件，用左侧按钮切换缓存，并用滑块转动主相机。右侧视口使用固定相机。

## glTF 材质对照

运行 `bazel run //examples/puzzle3d:play_materials` 或在 VS Code 中预览 [materials.xr](materials.xr)。从左到右，上排是哑光、清漆和带强度及粗糙度贴图的清漆。下排是基础发光、五倍发光和使用独立法线贴图的清漆。拖动滑块改变入射方向，开启 HDR 后可对比色调映射下的发光强度。

## glTF 薄壁玻璃

运行 `bazel run //examples/puzzle3d:play_transmission`，或在 VS Code 预览 [transmission.xr](transmission.xr)。左侧由 R 通道控制玻璃的透射量，右侧用粗糙度模糊并给背后的棋盘格着色。磨砂玻璃上半部经 alpha 裁剪镂空。开关可对比前景透明层和 HDR 合成。

## glTF 体积折射

运行 `bazel run //examples/puzzle3d:play_volume`，或在 VS Code 预览 [volume.xr](volume.xr)。两个玻璃块共用材质，右块的 glTF 节点沿深度轴放大两倍。拉动厚度滑块可以对照吸收程度，切换棋盘格和体积折射可查看背景的位移。

## 局部反射探针

运行 `bazel run //examples/puzzle3d:play_reflection_probes`，或在 VS Code 预览 [reflection_probes.xr](reflection_probes.xr)。球体从左到右使用冷色探针、重叠区域里的最近探针、暖色探针和全局 HDR。切换局部反射与视差校正，或转动镜头对照高光位置。

## 实时场景反射

运行 `bazel run //examples/puzzle3d:play_live_reflections`，或在 VS Code 预览 [live_reflections.xr](live_reflections.xr)。切换主相机背后的色卡，让球体上的反射随之改变。切到「固定首帧」后，点击「刷新探针」可显式更新立方体贴图。

右侧按钮在同帧更新、逐面更新和分级预滤波之间切换。分帧期间球体会显示上一张完整反射，直到新贴图完成。

## glTF 导入相机

运行 `bazel run //examples/puzzle3d:play_imported_cameras`，或在 VS Code 预览 [imported_cameras.xr](imported_cameras.xr)。切换透视、正交和无限远透视镜头，对照固定画幅、留边和远景裁剪。滑块调整透视镜头的 glTF 动画时间。

## 非对称视锥

运行 `bazel run //examples/puzzle3d:play_off_axis_camera`，或在 VS Code 预览 [off_axis_camera.xr](off_axis_camera.xr)。拖动滑块平移镜头的近裁剪面构图，点击边缘出现的方块验证拾取。用「居中」恢复对称视锥。

## glTF 导入布光

运行 `bazel run //examples/puzzle3d:play_punctual_lights`，或在 VS Code 预览 [punctual_lights.xr](punctual_lights.xr)。红色点光和绿色聚光灯由 glTF 动画驱动，蓝色方向光提供底色。滑块可拖动动画时间，也可切换导入灯光和手写补光。

## glTF 体积色散

运行 `bazel run //examples/puzzle3d:play_dispersion`，或在 VS Code 预览 [dispersion.xr](dispersion.xr)。切换「体积色散」对照同一玻璃块后的黑白条纹，拖动倾斜角度滑块查看红、绿、蓝光路的分离。

## glTF 介电镜面反射

运行 `bazel run //examples/puzzle3d:play_dielectric`，或在 VS Code 预览 [dielectric.xr](dielectric.xr)。上排比较四种折射率，下排展示反射强度和颜色贴图、镜面着色，以及纯金属对照。可切换直接光和 HDR 环境探针。

## glTF 纹理坐标

运行 `bazel run //examples/puzzle3d:play_uv_atlas`，或在 VS Code 预览 [uv_atlas.xr](uv_atlas.xr)。上方两张图集卡片使用同一纹理。右侧从另一组 UV 读取，并旋转、镜像和偏移图案，透明孔随新坐标移动。下方右侧镜像法线贴图，拖动滑块可以看到两边高光方向相反。

## 材质纹理缩小

运行 `bazel run //examples/puzzle3d:play_texture_mips`，或在 VS Code 预览 [texture_mips.xr](texture_mips.xr)。两张卡片共享一张奇数尺寸棋盘格，左边使用最近点，右边使用三线性 mipmap。移动距离和绕视滑块，对照两张卡片的远景纹理。

## 镜面高光抗锯齿

运行 `bazel run //examples/puzzle3d:play_specular_aa`，或在 VS Code 预览 [specular_aa.xr](specular_aa.xr)。从左到右依次是抛光金属、带清漆的半透明球体和无光照参考。拖动滑块移动镜头，对照开关前后的高光。选择「环境反射」切到 HDR 探针光照。

## 相交透明面

运行 `bazel run //examples/puzzle3d:play_transparency` 或在 VS Code 中预览 [transparency.xr](transparency.xr)。默认的子网格排序下，反转模型行会改变两张相交面片的混合结果。开启「加权透明」后可以比较相交处两侧的颜色，前方不透明盒体仍遮挡两张面片。HDR 和 4× MSAA 可独立切换。

## 屏幕空间环境遮蔽

运行 `bazel run //examples/puzzle3d:play_occlusion` 或在 VS Code 中预览 [occlusion.xr](occlusion.xr)。用「环境遮蔽」切换接触阴影，拖动右侧滑块调整采样半径。可以在透视、正交、HDR 和透明面片同时出现时比较效果。透明面片启用 4× MSAA 和加权透明。

## HDR 泛光

运行 `bazel run //examples/puzzle3d:play_bloom` 或在 VS Code 中预览 [bloom.xr](bloom.xr)。橙色发光片取自带有 glTF 发光强度扩展的模型。可以切换泛光、遮挡和前景玻璃，并拖动「强度」滑块。玻璃会同时启用 4× MSAA 和加权透明。

## 景深与焦平面

运行 `bazel run //examples/puzzle3d:play_focus` 或在 VS Code 中预览 [focus.xr](focus.xr)。三张棋盘格分别位于前景、焦平面和背景。拖动「对焦」滑块可以在三张卡片之间切换清晰区域。玻璃会同时启用 4× MSAA 和加权透明，HDR 会启用顶部发光点的泛光。

## 色调映射与颜色调整

运行 `bazel run //examples/puzzle3d:play_tone` 或在 VS Code 中预览 [tone.xr](tone.xr)。点击曲线名称切换左侧 HDR 视口的显示曲线，右侧保留同一组材质的 LDR 参考。亮度、对比度和饱和度滑块作用于两个视口。

## LUT 调色

运行 `bazel run //examples/puzzle3d:play_lut` 或在 VS Code 中预览 [lut.xr](lut.xr)。点击预设名称切换原色、恒等、暖调和反相，用「混合」滑块调整比例。选择「HDR + 泛光」可观察同一张 LUT 如何作用于 HDR 合成的最终颜色。

## 高度图地形

```sh
bazel run //examples/puzzle3d:play_heightfield
```

用 `@mesh-url("models/ridge.png", lightmap-uv, auto-lod)` 加载 16 位灰度地形。点地形会把金色标记放到命中点。拖动滑块调整视角，勾选「压平地形」会改变高度，标记按射线查询结果贴着地面移动。在 VS Code 中可直接预览 `heightfield.xr`。

## 山谷晨雾

```sh
bazel run //examples/puzzle3d:play_fog
```

对照同一山谷中的高低与远近标记。可独立切换高度雾和距离雾、调节曲线、倒置高度范围，并开启亮部透射。VS Code 可实时预览 `fog.xr`。

## 局部体积雾

```sh
bazel run //examples/puzzle3d:play_volumetric_fog
```

示例用一片蓝色盒形雾和一片琥珀色椭球雾，对照雾前、雾中和雾后的物体。可切换雾体与几何显示，拖动滑块绕视。VS Code 可实时预览 `volumetric_fog.xr`。

## 体积光与雾影

```sh
bazel run //examples/puzzle3d:play_fog_lighting
```

切换点光源、方向光和聚光灯，拖动滑块升降灯具。开启雾影后，挡板会在雾中留下暗区。VS Code 可直接预览 `fog_lighting.xr`。

## 雾体旋转与高度衰减

```sh
bazel run //examples/puzzle3d:play_fog_profiles
```

拖动右侧滑块旋转盒形雾体，切换高度衰减与倒置方向。重叠的暖色椭球有自己的世界高度范围。VS Code 可直接预览 `fog_profiles.xr`。

## 流动体积雾

```sh
bazel run //examples/puzzle3d:play_fog_noise
```

切换「雾团」对照均匀雾与三维噪声，调节滑块移动噪声，或打开「随风」开始动画。示例把高度衰减、体积光和遮挡叠加在同一雾体上。VS Code 可直接预览 `fog_noise.xr`。

## 球体扫掠

```sh
bazel run //examples/puzzle3d:play_sphere_cast
```

改变球体半径和请求移动距离，观察球体在墙边停住。关闭「墙体阻挡」后球体可以通过，墙体仍可拾取。示例同时查询高度图接触点与重叠记录。VS Code 可直接预览 `sphere_cast.xr`。

## 沿墙滑移

```sh
bazel run //examples/puzzle3d:play_sphere_slide
```

黄色球标出斜向位移经墙体约束后的终点。切换「拐角墙」查看沿墙滑行的长度，按「走一步」提交位移。方向键与 WASD 可推动青色球，右侧两个滑块调整预览位移。VS Code 可直接预览 `sphere_slide.xr`。

## 地面行走

```sh
bazel run //examples/puzzle3d:play_sphere_walk
```

黄色球显示下一次移动的落点。切换「上台阶」对照矮台阶，切换「贴地」对照下台阶后的落点，按「走一步」提交位移。方向键与 WASD 可控制球沿平台和斜坡移动。VS Code 可直接预览 `sphere_walk.xr`。

## 3D 粒子

```sh
bazel run //examples/puzzle3d:play_particles
```

左侧喷泉连续发射橙色粒子，右侧按钮触发青色爆发。拖动滑块可在暂停时选定任意一帧。切换遮挡、加权透明、HDR 和 4× MSAA 可以对照渲染效果。VS Code 预览 `particles.xr` 时，可以边调整初速度或颜色边观察同一时刻的粒子分布。

## 粒子序列帧

```sh
bazel run //examples/puzzle3d:play_sprite_particles
```

左侧精灵展示横排 PNG 的当前帧，中间展示单帧贴图，右侧让不同粒子以不同相位和速度播放同一张图。拖动时间轴对照循环、倒放、往返和插值。在 VS Code 预览 `sprite_particles.xr` 时，可以调整贴图、播放周期和起始帧。

## 粒子混合与颜色表

```sh
bazel run //examples/puzzle3d:play_particle_materials
```

左侧按钮在普通混合、加色、滤色和正片叠底之间切换。拖动寿命滑块可观察颜色表对软圆精灵及环形贴图的颜色和透明度影响。右侧粒子按各自的种子选取颜色表中的行。VS Code 可以预览 `particle_materials.xr` 并在修改 PNG 后刷新效果。

## 空间粒子发射

```sh
bazel run //examples/puzzle3d:play_particle_emitters
```

左侧切换盒体、球体和圆柱，选择从体积还是表面发射，再用滑块旋转它。右侧切换锥形、目标和定速向量模式。拖时间轴可观察每颗粒子朝目标靠拢的轨迹。VS Code 可直接预览 `particle_emitters.xr`。

## 网格表面发射

```sh
bazel run //examples/puzzle3d:play_particle_mesh_emitters
```

示例的 glTF 有两个面积不同、朝向不同的三角面。上方显示出生位置，下方显示沿各面法线分开的粒子轨迹。可以切到目标方向、盒体网格，或者调整旋转、缩放和时间。VS Code 可直接预览 `particle_mesh_emitters.xr`，也可编辑 glTF 并实时查看结果。

## 粒子影响器

```sh
bazel run //examples/puzzle3d:play_particle_affectors
```

蓝色粒子受影响器控制，橙色粒子沿原轨迹运动。点击左侧按钮切换吸引、排斥、重力和绕点旋转。拖动时间滑块查看同一粒子的不同年龄。开启「叠加重力」后，交换影响器顺序可以看到先吸引再下落和最后集中在目标两种结果。VS Code 可以直接预览 `particle_affectors.xr`。
