//! Common item code.
//!
//! An item is anything in the game that can be picked up by the player.

use crate::prelude::*;

pub fn install(session: &mut SessionBuilder) {
    Item::register_schema();
    ItemThrow::register_schema();
    ItemGrab::register_schema();
    DropItem::register_schema();
    ItemUsed::register_schema();

    session
        .stages
        .add_system_to_stage(CoreStage::Last, grab_items)
        .add_system_to_stage(CoreStage::Last, drop_items)
        .add_system_to_stage(CoreStage::Last, throw_dropped_items);
}

/// Marker component for items.
///
/// Items are any entity that players can pick up and use.
#[derive(Clone, Copy, HasSchema, Default)]
#[repr(C)]
pub struct Item;

/// An intventory component, indicating another entity that the player is carrying.
#[derive(Clone, HasSchema, Default, Deref, DerefMut)]
pub struct Inventory(pub Option<Entity>);

/// Marker component that may be added to an item to cause it to be droped by a player.
#[derive(Clone, HasSchema, Default)]
#[repr(C)]
pub struct DropItem;

/// A helper struct containing a player-inventory pair that indicates the given player is holding
/// the other entity in their inventory.
#[derive(Debug, Clone, Copy)]
pub struct Inv {
    pub player: Entity,
    pub inventory: Entity,
}

/// System param that can be used to conveniently get the inventory of each player.
#[derive(Deref, DerefMut, Debug)]
pub struct PlayerInventories<'a>(&'a [Option<Inv>; MAX_PLAYERS as usize]);

impl PlayerInventories<'_> {
    pub fn find_item(&self, item: Entity) -> Option<Inv> {
        self.0
            .iter()
            .find_map(|i| i.filter(|inv| inv.inventory == item))
    }
}

impl<'a> SystemParam for PlayerInventories<'a> {
    type State = [Option<Inv>; MAX_PLAYERS as usize];
    type Param<'s> = PlayerInventories<'s>;

    fn get_state(world: &World) -> Self::State {
        world.run_system(
            |entities: Res<Entities>,
             player_indexes: Comp<PlayerIdx>,
             inventories: Comp<Inventory>| {
                let mut player_inventories = [None; MAX_PLAYERS as usize];
                for (player, (idx, inventory)) in
                    entities.iter_with((&player_indexes, &inventories))
                {
                    if let Some(inventory) = inventory.0 {
                        player_inventories[idx.0 as usize] = Some(Inv { player, inventory });
                    }
                }

                player_inventories
            },
            (),
        )
    }

    fn borrow<'s>(_world: &'s World, state: &'s mut Self::State) -> Self::Param<'s> {
        PlayerInventories(state)
    }
}

/// Marker component added to items when they are dropped.
#[derive(Clone, Copy, HasSchema, Default)]
pub struct ItemDropped {
    /// The player that dropped the item
    pub player: Entity,
}

/// Marker component added to items when they are grabbed.
#[derive(Clone, Copy, HasSchema, Default)]
pub struct ItemGrabbed {
    /// The player that grabbed the item
    pub player: Entity,
}

/// Marker component added to items when they are used.
#[derive(Clone, Copy, HasSchema, Default)]
#[repr(C)]
pub struct ItemUsed {
    /// The player that used the item
    pub owner: Entity,
}

/// Component defining the grab settings when an item is grabbed.
///
/// Mainly handled by the [`grab_items`] system which consumes the
/// [`ItemGrabbed`] components for entities which have this component.
/// [`Item`] is required for the system to take affect.
#[derive(Clone, HasSchema, Default)]
#[repr(C)]
pub struct ItemGrab {
    pub fin_anim: Ustr,
    pub grab_offset: Vec2,
    pub sync_animation: bool,
}

/// Drop items that have the `DropItem` component added to them.
pub fn drop_items(
    mut commands: Commands,
    mut drop_items: CompMut<DropItem>,
    player_inventories: PlayerInventories,
) {
    for Inv { player, inventory } in player_inventories.iter().flatten() {
        if drop_items.remove(*inventory).is_some() {
            commands.add(PlayerCommand::set_inventory(*player, None));
        }
    }
}

pub fn grab_items(
    entities: Res<Entities>,
    item_grab: Comp<ItemGrab>,
    items: Comp<Item>,
    mut items_grabbed: CompMut<ItemGrabbed>,
    mut bodies: CompMut<KinematicBody>,
    mut attachments: CompMut<PlayerBodyAttachment>,
    mut player_layers: CompMut<PlayerLayers>,
) {
    for (entity, (_item, item_grab)) in entities.iter_with((&items, &item_grab)) {
        let ItemGrab {
            fin_anim,
            grab_offset,
            sync_animation,
        } = *item_grab;

        if let Some(ItemGrabbed { player }) = items_grabbed.remove(entity) {
            player_layers.get_mut(player).unwrap().fin_anim = fin_anim;

            if let Some(body) = bodies.get_mut(entity) {
                body.is_deactivated = true
            }

            attachments.insert(
                entity,
                PlayerBodyAttachment {
                    player,
                    sync_animation,
                    sync_color: false,
                    head: false,
                    offset: grab_offset.extend(PlayerLayers::FIN_Z_OFFSET / 2.0),
                },
            );
        }
    }
}

/// Component defining the strength of the throw types when an item is dropped.
///
/// Mainly handled by the [`throw_dropped_items`] system which consumes the
/// [`ItemDropped`] components for entities which have this component.
/// [`Item`] is required for the system to take affect.
#[derive(Clone, HasSchema)]
#[repr(C)]
pub struct ItemThrow {
    normal: Vec2,
    fast: Vec2,
    up: Vec2,
    drop: Vec2,
    lob: Vec2,
    roll: Vec2,
    spin: f32,
    #[schema(opaque)]
    /// An optional system value that gets run once on throw.
    system: Option<Arc<AtomicCell<StaticSystem<(), ()>>>>,
}

impl Default for ItemThrow {
    fn default() -> Self {
        Self::base()
    }
}

impl ItemThrow {
    /// The relative velocities of each different throw type.
    ///
    /// This is multiiplied by the desired throw strength in [`Self::strength`] to get a deafault
    /// throw pattern from a single velocity.
    pub fn base() -> Self {
        Self {
            normal: Vec2::new(1.5, 1.2).normalize() * 0.6,
            fast: Vec2::new(1.5, 1.2).normalize(),
            up: Vec2::new(0.0, 1.1),
            drop: Vec2::new(0.0, 0.0),
            lob: Vec2::new(1.0, 2.5).normalize() * 1.1,
            roll: Vec2::new(0.4, -0.1),
            spin: 0.0,
            system: None,
        }
    }

    /// [`Self::base`] with the throw values multiplied by `strength`.
    pub fn strength(strength: f32) -> Self {
        let base = Self::base();
        Self {
            normal: base.normal * strength,
            fast: base.fast * strength,
            up: base.up * strength,
            drop: base.drop * strength,
            lob: base.lob * strength,
            roll: base.roll * strength,
            spin: 0.0,
            system: None,
        }
    }

    pub fn with_spin(self, spin: f32) -> Self {
        Self { spin, ..self }
    }

    pub fn with_system<Args, I>(self, system: I) -> Self
    where
        I: IntoSystem<Args, (), (), Sys = StaticSystem<(), ()>>,
    {
        Self {
            system: Some(Arc::new(AtomicCell::new(system.system()))),
            ..self
        }
    }

    /// Chooses one of the throw values based on a [`PlayerControl`]
    pub fn velocity_from_control(&self, player_control: &PlayerControl) -> Vec2 {
        let PlayerControl { move_direction, .. } = player_control;
        let y = move_direction.y;
        let moving = move_direction.x.abs() > 0.0;
        if y < 0.0 {
            if moving {
                return self.roll;
            } else {
                return self.drop;
            }
        }
        if moving {
            if y > 0.0 {
                self.lob
            } else {
                self.fast
            }
        } else if y > 0.0 {
            self.up
        } else {
            self.normal
        }
    }
}

pub fn throw_dropped_items(
    entities: Res<Entities>,
    item_throws: Comp<ItemThrow>,
    items: Comp<Item>,
    player_inputs: Res<MatchInputs>,
    player_indexes: Comp<PlayerIdx>,
    mut items_dropped: CompMut<ItemDropped>,
    mut bodies: CompMut<KinematicBody>,
    mut attachments: CompMut<PlayerBodyAttachment>,
    mut sprites: CompMut<AtlasSprite>,
    mut transforms: CompMut<Transform>,
    item_spawners: Comp<DehydrateOutOfBounds>,
    map_layers: Comp<SpawnedMapLayerMeta>,
    player_spawnwers: Comp<PlayerSpawner>,
    mut commands: Commands,
) {
    for (entity, (_items, item_throw, transform)) in
        entities.iter_with((&items, &item_throws, &mut transforms))
    {
        if let Some(ItemDropped { player }) = items_dropped.get(entity).cloned() {
            if let Some(system) = item_throw.system.clone() {
                commands.add(move |world: &World| (system.borrow_mut().run)(world, ()));
            }
            items_dropped.remove(entity);
            attachments.remove(entity);

            let player_sprite = sprites.get_mut(player).unwrap();

            let horizontal_flip_factor = if player_sprite.flip_x {
                Vec2::new(-1.0, 1.0)
            } else {
                Vec2::ONE
            };

            let throw_velocity = item_throw.velocity_from_control(
                &player_inputs.players[player_indexes.get(player).unwrap().0 as usize].control,
            );

            // Use the item's spawner depth as the drop depth
            if let Some(item_spawner) = item_spawners.get(entity) {
                let map_layer = map_layers.get(item_spawner.0).unwrap();
                transform.translation.z = z_depth_for_map_layer(map_layer.layer_idx);
            } else {
                // Grab a random player spawner and use that for the z depth
                let (_, (_, layer)) = entities
                    .iter_with((&player_spawnwers, &map_layers))
                    .next()
                    .unwrap();
                transform.translation.z = z_depth_for_map_layer(layer.layer_idx);
            }

            if let Some(body) = bodies.get_mut(entity) {
                body.velocity = throw_velocity * horizontal_flip_factor;
                body.angular_velocity =
                    item_throw.spin * horizontal_flip_factor.x * throw_velocity.y.signum();

                body.is_deactivated = false;
            }
        }
    }
}
