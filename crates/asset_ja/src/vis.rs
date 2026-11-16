use fmt_ja::bsp::{self, BspError, BspLeaf, BspNode, BspPlane, BspVisibility};

/// Jedi Academy BSP visibility and spatial metadata.
///
/// Surface indices refer to the original BSP surface lump, not the
/// converted ModelIr mesh indices.
#[derive(Debug, Clone)]
pub struct JaVisibility {
    planes: Vec<BspPlane>,
    nodes: Vec<BspNode>,
    leaves: Vec<BspLeaf>,
    pvs: Option<BspVisibility>,
    surface_clusters: Vec<Vec<i32>>,
}

impl JaVisibility {
    /// Parse the spatial and visibility information from an RBSP file.
    pub fn parse(bytes: &[u8]) -> Result<Self, BspError> {
        let parsed = bsp::parse(bytes)?;

        let planes = bsp::parse_planes(bytes, &parsed)?;
        let nodes = bsp::parse_nodes(bytes, &parsed)?;
        let leaves = bsp::parse_leaves(bytes, &parsed)?;
        let leaf_surfaces = bsp::parse_leaf_surfaces(bytes, &parsed)?;
        let surfaces = bsp::parse_surfaces(bytes, &parsed)?;
        let pvs = bsp::parse_visibility(bytes, &parsed)?;

        let mut surface_clusters = vec![Vec::new(); surfaces.len()];

        for (leaf_index, leaf) in leaves.iter().enumerate() {
            let start = usize::try_from(leaf.first_leaf_surface).map_err(|_| {
                BspError::Bad(format!(
                    "leaf {leaf_index} has a negative first leaf-surface index"
                ))
            })?;

            let count = usize::try_from(leaf.num_leaf_surfaces).map_err(|_| {
                BspError::Bad(format!(
                    "leaf {leaf_index} has a negative leaf-surface count"
                ))
            })?;

            let end = start.checked_add(count).ok_or_else(|| {
                BspError::Bad(format!("leaf {leaf_index} leaf-surface range overflows"))
            })?;

            let references = leaf_surfaces.get(start..end).ok_or_else(|| {
                BspError::Bad(format!(
                    "leaf {leaf_index} leaf-surface range is out of bounds"
                ))
            })?;

            // Negative clusters represent leaves outside the PVS.
            if leaf.cluster < 0 {
                continue;
            }

            if let Some(pvs) = &pvs {
                if usize::try_from(leaf.cluster).map_or(true, |cluster| cluster >= pvs.num_clusters)
                {
                    return Err(BspError::Bad(format!(
                        "leaf {leaf_index} has out-of-range cluster {}",
                        leaf.cluster
                    )));
                }
            }

            for &surface_index in references {
                let surface_index = usize::try_from(surface_index).map_err(|_| {
                    BspError::Bad(format!(
                        "leaf {leaf_index} references a negative surface index"
                    ))
                })?;

                let clusters = surface_clusters.get_mut(surface_index).ok_or_else(|| {
                    BspError::Bad(format!(
                        "leaf {leaf_index} references out-of-range surface {surface_index}"
                    ))
                })?;

                if !clusters.contains(&leaf.cluster) {
                    clusters.push(leaf.cluster);
                }
            }
        }

        Ok(Self {
            planes,
            nodes,
            leaves,
            pvs,
            surface_clusters,
        })
    }

    /// Find the BSP leaf containing a point and return its cluster.
    ///
    /// Coordinates must be in original Quake/Jedi Academy BSP units,
    /// before applying the asset renderer's UNIT conversion.
    pub fn cluster_for_point(&self, point: [f32; 3]) -> Option<i32> {
        if !point.iter().all(|value| value.is_finite()) {
            return None;
        }

        // BSP node zero is the root.
        let mut node_index = 0usize;

        // A valid tree cannot require more node visits than this.
        // The bound also prevents malformed cyclic data from looping.
        for _ in 0..=self.nodes.len() {
            let node = self.nodes.get(node_index)?;

            let plane_index = usize::try_from(node.plane).ok()?;
            let plane = self.planes.get(plane_index)?;

            let distance = point[0] * plane.normal[0]
                + point[1] * plane.normal[1]
                + point[2] * plane.normal[2]
                - plane.dist;

            if !distance.is_finite() {
                return None;
            }

            let child = node.children[usize::from(distance < 0.0)];

            if child >= 0 {
                node_index = usize::try_from(child).ok()?;
                continue;
            }

            // Negative children encode leaves as -index - 1.
            let leaf_index = usize::try_from(-i64::from(child) - 1).ok()?;
            let leaf = self.leaves.get(leaf_index)?;

            return (leaf.cluster >= 0).then_some(leaf.cluster);
        }

        None
    }

    /// Is an original BSP surface potentially visible from this cluster?
    ///
    /// Missing PVS, invalid source clusters and surfaces without cluster
    /// associations all fail open.
    pub fn is_surface_visible(&self, source_cluster: i32, surface_index: usize) -> bool {
        let Some(pvs) = &self.pvs else {
            return true;
        };

        if source_cluster < 0
            || usize::try_from(source_cluster).map_or(true, |cluster| cluster >= pvs.num_clusters)
        {
            return true;
        }

        let Some(clusters) = self.surface_clusters.get(surface_index) else {
            return true;
        };

        if clusters.is_empty() {
            return true;
        }

        clusters
            .iter()
            .any(|&target_cluster| pvs.is_visible(source_cluster, target_cluster))
    }

    /// Is a surface potentially visible from a world-space point?
    pub fn is_surface_visible_from(&self, point: [f32; 3], surface_index: usize) -> bool {
        let Some(cluster) = self.cluster_for_point(point) else {
            return true;
        };

        self.is_surface_visible(cluster, surface_index)
    }

    /// Return the clusters associated with an original BSP surface.
    pub fn surface_clusters(&self, surface_index: usize) -> &[i32] {
        self.surface_clusters
            .get(surface_index)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Number of original BSP surfaces represented by this visibility map.
    pub fn surface_count(&self) -> usize {
        self.surface_clusters.len()
    }

    /// Number of PVS clusters, or zero when the BSP has no visibility lump.
    pub fn cluster_count(&self) -> usize {
        self.pvs.as_ref().map_or(0, |pvs| pvs.num_clusters)
    }

    /// Whether the BSP contains a visibility lump.
    pub fn has_pvs(&self) -> bool {
        self.pvs.is_some()
    }
}
