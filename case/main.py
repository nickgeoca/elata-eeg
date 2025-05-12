import cadquery as cq
import pi5 as pi5
import lcd as lcd
import ads1299evm as ads

def make_backplate(lcd_pos, z_top, plate_thickness):
    pts = [pt for pt in lcd_pos.outer_mount_holes]
    xs = [p.x for p in pts]
    ys = [p.y for p in pts]

    # Bounding box based on screw holes
    screw_margin = 2.5
    bbox_w = (max(xs) - min(xs)) + 2 * screw_margin
    bbox_h = (max(ys) - min(ys)) + 2 * screw_margin

    # 1. Flat Pi-covering plate
    base_plate = (
        cq.Workplane("XY", origin=(0, 0, z_top))
        .rect(bbox_w, bbox_h)
        .extrude(-plate_thickness)
    )

    # 2. Cut screw holes
    screw_holes = (
        cq.Workplane("XY", origin=(0, 0, z_top))
        .pushPoints(pts)
        .circle(lcd.MOUNT_HOLE_DIAMETER / 2)
        .extrude(-plate_thickness - 0.1)
    )
    plate_with_holes = base_plate.cut(screw_holes)

    # 3. Add corner ramps (simplified as four angled blocks)
    ramp_depth = z_top  # assuming gusset Z = 0
    ramp_size = 4.0     # size of each triangular ramp corner block

    ramps = []
    for pt in pts:
        x, y = pt.x, pt.y
        ramp = (
            cq.Workplane("XY", origin=(x, y, 0))
            .polyline([(0, 0), (ramp_size, 0), (0, ramp_size)])
            .close()
            .extrude(ramp_depth)
        )
        ramps.append(ramp)

    final = plate_with_holes
    for ramp in ramps:
        final = final.union(ramp)   
    return final


def case_add_lcd_shell(lcd_pos, pcb_margin, case_thickness):
    # Get LCD glass size
    top_pts = lcd_pos.top_panel
    glass_w = max(v.x for v in top_pts) - min(v.x for v in top_pts)
    glass_h = max(v.y for v in top_pts) - min(v.y for v in top_pts)
    print(glass_w, glass_h)
    # Get PCB footprint
    pcb_pts = lcd_pos.bottom_pcb
    pcb_w = max(v.x for v in pcb_pts) - min(v.x for v in pcb_pts)
    pcb_h = max(v.y for v in pcb_pts) - min(v.y for v in pcb_pts)

    # Inner case bounds (room for PCB + margin)
    inner_w = pcb_w + 2 * pcb_margin
    inner_h = pcb_h + 2 * pcb_margin

    # Outer case bounds (add wall thickness)
    outer_w = inner_w + 2 * case_thickness
    outer_h = inner_h + 2 * case_thickness

    # ----- Lip: top rim with touchscreen cutout -----
    lip = (
        cq.Workplane("XY")
        .rect(inner_w, inner_h)
        .workplane()                 # Stay on the same plane
        .center(0, (lcd.PCB_HEIGHT / 2) - (lcd.PANEL_HEIGHT / 2))
        .rect(glass_w, glass_h)
        .extrude(-case_thickness)
    )

    # ----- Skirt: bottom shell (walls around PCB) -----
    top_z = 0
    extend_to_stud_mounts = -(top_z + lcd.PANEL_THICKNESS + lcd.PCB_THICKNESS + lcd.STUD_HEIGHT + case_thickness) # extends to the trinagles holding LCD in place
    skirt = (
        cq.Workplane("XY")
        .workplane(offset=top_z)
        .rect(outer_w, outer_h)
        .extrude(extend_to_stud_mounts)
        .faces(">Z")
        .workplane()
        .rect(inner_w, inner_h)
        .cutBlind(extend_to_stud_mounts)
    )

    # ----- Gusset Triangles -----
    gussets = cq.Workplane("XY")

    triangle_size = 22  # bigger triangle
    gusset_depth = case_thickness  # thickness into the case wall

    # Each corner with its triangle direction
    corners = [
        { "pos": (-inner_w / 2, -inner_h / 2), "shape": [(0, 0), (triangle_size, 0), (0, triangle_size)] },  # bottom left
        { "pos": ( inner_w / 2, -inner_h / 2), "shape": [(0, 0), (-triangle_size, 0), (0, triangle_size)] }, # bottom right
        { "pos": ( inner_w / 2,  inner_h / 2), "shape": [(0, 0), (-triangle_size, 0), (0, -triangle_size)] },# top right
        { "pos": (-inner_w / 2,  inner_h / 2), "shape": [(0, 0), (triangle_size, 0), (0, -triangle_size)] }, # top left
    ]

    for corner, pt in zip(corners, lcd_pos.outer_mount_holes):
        base_z = pt.z - lcd.STUD_HEIGHT
        gusset = (
            cq.Workplane("XY")
            .center(*corner["pos"])
            .workplane(offset=base_z)
            .polyline(corner["shape"])
            .close()
            .extrude(-gusset_depth)
        )

        gussets = gussets.union(gusset)

    gusset_holes = (
        cq.Workplane("XY")
        .workplane(offset=0)  # Assuming base is at Z = 0
        .pushPoints([pt.toTuple() for pt in lcd_pos.outer_mount_holes])
        .circle(lcd.MOUNT_HOLE_DIAMETER / 2)
        .extrude(-case_thickness - lcd.STUD_HEIGHT)  # Or -STUD_HEIGHT if just through gussets
    )

    z_offset = -40  # or wherever your back panel sits
    # back_plate = make_backplate(lcd_pos, z_offset, case_thickness)

    return skirt.union(lip).union(gussets.cut(gusset_holes))

def main():
    pi5_pos = pi5.TransformedPositions()
    lcd_pos = lcd.TransformedPositions()
    ads_pos = ads.TransformedPositions()
    # <---- Move and line up the components ----> #

    mount_pi5_to_lcd_and_rotate = (cq.Vector(-1.5, 7, -11.6), cq.Vector(1, 0, 0), -90)
    pi5_pos.update(cq.Location(*mount_pi5_to_lcd_and_rotate))

    position_adc = (cq.Vector(0, -90, -20), cq.Vector(0, 0, 1), 90)
    ads_pos.update(cq.Location(*position_adc))

    case_thickness = 3.0
    component_margin = 3.0 # extra around the PCB and the awkward DSI cable on the edge
    case = case_add_lcd_shell(lcd_pos, component_margin, case_thickness)

    case.

    # <----            Assemble             ----> #
    main_assembly = cq.Assembly()
    main_assembly.add(pi5.create(), name="pi5_instance", loc=pi5_pos.xyzθ)
    main_assembly.add(lcd.create(), name="lcd_instance", loc=lcd_pos.xyzθ)
    main_assembly.add(ads.create(), name="ads_instance", loc=ads_pos.xyzθ)
    main_assembly.add(case        , name="case"        , loc=)

    print(f"  FINAL Pi 5 xyz: { pi5_pos.xyzθ.toTuple()[0]}")
    print(f"  FINAL ADS1299evm xyz: { ads_pos.xyzθ.toTuple()[0]}")
    print(f"  FINAL LCD xyz: { lcd_pos.xyzθ.toTuple()[0]}")
    main_assembly.save("eeg.step")
    return main_assembly