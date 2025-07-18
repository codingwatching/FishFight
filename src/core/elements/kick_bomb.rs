use crate::prelude::*;

#[derive(HasSchema, Default, Debug, Clone)]
#[type_data(metadata_asset("kick_bomb"))]
#[repr(C)]
pub struct KickBombMeta {
    pub body_diameter: f32,
    pub fin_anim: Ustr,
    pub grab_offset: Vec2,
    pub damage_region_size: Vec2,
    pub damage_region_lifetime: f32,
    pub kick_velocity: Vec2,
    pub kickable: bool,
    pub throw_velocity: f32,
    pub explosion_lifetime: f32,
    pub explosion_frames: u32,
    pub explosion_fps: f32,
    pub explosion_sound: Handle<AudioSource>,
    pub explosion_volume: f64,
    pub lit_frames_start: u32,
    pub lit_frames_end: u32,
    pub lit_fps: f32,
    pub fuse_sound: Handle<AudioSource>,
    pub fuse_sound_volume: f64,
    /// The time in seconds before a grenade explodes
    pub fuse_time: Duration,
    pub can_rotate: bool,
    /// The grenade atlas
    pub atlas: Handle<Atlas>,
    pub explosion_atlas: Handle<Atlas>,
    pub bounciness: f32,
    pub angular_velocity: f32,
    pub arm_delay: Duration,
    pub explode_on_contact: bool,
}

pub fn game_plugin(_game: &mut Game) {
    KickBombMeta::register_schema();
}

pub fn session_plugin(session: &mut SessionBuilder) {
    session
        .stages
        .add_system_to_stage(CoreStage::PreUpdate, hydrate)
        .add_system_to_stage(CoreStage::PostUpdate, update_lit_kick_bombs)
        .add_system_to_stage(CoreStage::PostUpdate, update_idle_kick_bombs);
}

#[derive(Clone, HasSchema, Default, Debug, Copy)]
pub struct IdleKickBomb;

#[derive(Clone, HasSchema, Default, Debug)]
pub struct LitKickBomb {
    arm_delay: Timer,
    fuse_time: Timer,
    kicking: bool,
    kicks: u32,
}

/// Component containing the kick bombs's metadata handle.
#[derive(Deref, DerefMut, HasSchema, Default, Clone)]
#[repr(C)]
pub struct KickBombHandle(pub Handle<KickBombMeta>);

/// Commands for KickBombs
#[derive(Clone, Debug)]
pub struct KickBombCommand;

impl KickBombCommand {
    /// Command for spawning  a kick bomb.
    /// If entity is provided, components are added to this entity, otherwise command spawns the entity
    ///
    /// `kick_bomb_handle` must cast to `Handle<KickBombMeta>` or `Handle<ElementMeta>` where [`ElementMeta`]
    /// contains handle that casts to `Handle<KickBombMeta>`.
    /// [`Handle::untyped`] should be used to convert to [`UntypedHandle`].
    #[must_use]
    pub fn spawn_kick_bomb(
        entity: Option<Entity>,
        transform: Transform,
        kick_bomb_meta_handle: UntypedHandle,
        lit: bool,
        player_flip_f: Option<bool>,
    ) -> StaticSystem<(), ()> {
        (move |game_meta: Root<GameMeta>,
               assets: Res<AssetServer>,
               mut animated_sprites: CompMut<AnimatedSprite>,
               mut atlas_sprites: CompMut<AtlasSprite>,
               mut bodies: CompMut<KinematicBody>,
               mut entities: ResMutInit<Entities>,
               mut idle_bombs: CompMut<IdleKickBomb>,
               mut lit_bombs: CompMut<LitKickBomb>,
               mut items: CompMut<Item>,
               mut item_throws: CompMut<ItemThrow>,
               mut item_grabs: CompMut<ItemGrab>,
               mut kick_bomb_handles: CompMut<KickBombHandle>,
               mut transforms: CompMut<Transform>| {
            // Unwrap entity or spawn if existing entity was not provided.
            let entity = entity.unwrap_or_else(|| entities.create());

            // Try to use handle as Handle<KickBombMeta>.
            let kick_bomb_meta_handle =
                match try_cast_meta_handle::<KickBombMeta>(kick_bomb_meta_handle, &assets) {
                    Ok(handle) => handle,
                    Err(err) => {
                        error!("KickBombCommand::spawn_kick_bomb() failed: {err}");
                        return;
                    }
                };

            let KickBombMeta {
                atlas,
                fin_anim,
                grab_offset,
                body_diameter,
                can_rotate,
                bounciness,
                throw_velocity,
                angular_velocity,
                arm_delay,
                fuse_time,
                ..
            } = *assets.get(kick_bomb_meta_handle);

            kick_bomb_handles.insert(entity, KickBombHandle(kick_bomb_meta_handle));
            items.insert(entity, Item);
            item_throws.insert(
                entity,
                ItemThrow::strength(throw_velocity).with_spin(angular_velocity),
            );
            item_grabs.insert(
                entity,
                ItemGrab {
                    fin_anim,
                    sync_animation: false,
                    grab_offset,
                },
            );
            atlas_sprites.insert(entity, AtlasSprite::new(atlas));
            transforms.insert(entity, transform);
            animated_sprites.insert(entity, default());
            bodies.insert(
                entity,
                KinematicBody {
                    shape: ColliderShape::Circle {
                        diameter: body_diameter,
                    },
                    gravity: game_meta.core.physics.gravity,
                    has_mass: true,
                    has_friction: true,
                    can_rotate,
                    bounciness,
                    ..default()
                },
            );

            if lit {
                lit_bombs.insert(
                    entity,
                    LitKickBomb {
                        arm_delay: Timer::new(arm_delay, TimerMode::Once),
                        fuse_time: Timer::new(fuse_time, TimerMode::Once),
                        kicking: false,
                        kicks: 0,
                    },
                );

                if let Some(body) = bodies.get_mut(entity) {
                    let horizontal_flip_factor = if player_flip_f.unwrap() {
                        Vec2::new(-1.0, 1.0)
                    } else {
                        Vec2::ONE
                    };

                    body.velocity = Vec2::new(
                        horizontal_flip_factor.x * throw_velocity,
                        horizontal_flip_factor.y * throw_velocity / 2.5,
                    );
                    body.angular_velocity = angular_velocity;
                }
            } else {
                idle_bombs.insert(entity, IdleKickBomb);
            }
        })
        .system()
    }
}

fn hydrate(
    assets: Res<AssetServer>,
    mut entities: ResMutInit<Entities>,
    transforms: Comp<Transform>,
    mut hydrated: CompMut<MapElementHydrated>,
    mut element_handles: CompMut<ElementHandle>,
    mut respawn_points: CompMut<DehydrateOutOfBounds>,
    mut spawner_manager: SpawnerManager,
    mut commands: Commands,
) {
    let mut not_hydrated_bitset = hydrated.bitset().clone();
    not_hydrated_bitset.bit_not();
    not_hydrated_bitset.bit_and(element_handles.bitset());

    let spawner_entities = entities
        .iter_with_bitset(&not_hydrated_bitset)
        .collect::<Vec<_>>();

    for spawner_ent in spawner_entities {
        let transform = *transforms.get(spawner_ent).unwrap();
        let element_handle = *element_handles.get(spawner_ent).unwrap();

        // Check if spawner element handle is for kick bomb
        let element_meta = assets.get(element_handle.0);
        if assets
            .get(element_meta.data)
            .try_cast_ref::<KickBombMeta>()
            .is_ok()
        {
            hydrated.insert(spawner_ent, MapElementHydrated);

            let entity = entities.create();
            hydrated.insert(entity, MapElementHydrated);
            element_handles.insert(entity, element_handle);
            respawn_points.insert(entity, DehydrateOutOfBounds(spawner_ent));
            spawner_manager.create_spawner(spawner_ent, vec![entity]);

            commands.add(KickBombCommand::spawn_kick_bomb(
                Some(entity),
                transform,
                element_meta.data.untyped(),
                false,
                None,
            ));
        }
    }
}

fn update_idle_kick_bombs(
    entities: Res<Entities>,
    mut commands: Commands,
    mut items_used: CompMut<ItemUsed>,
    mut audio_center: ResMut<AudioCenter>,
    kick_bomb_handles: Comp<KickBombHandle>,
    mut idle_bombs: CompMut<IdleKickBomb>,
    assets: Res<AssetServer>,
    mut animated_sprites: CompMut<AnimatedSprite>,
) {
    for (entity, (_kick_bomb, kick_bomb_handle)) in
        entities.iter_with((&mut idle_bombs, &kick_bomb_handles))
    {
        let kick_bomb_meta = assets.get(kick_bomb_handle.0);

        let KickBombMeta {
            fuse_sound,
            fuse_sound_volume,
            arm_delay,
            fuse_time,
            lit_frames_start,
            lit_frames_end,
            lit_fps,
            ..
        } = *kick_bomb_meta;

        if items_used.remove(entity).is_some() {
            audio_center.play_sound(fuse_sound, fuse_sound_volume);
            let animated_sprite = animated_sprites.get_mut(entity).unwrap();
            animated_sprite.frames = (lit_frames_start..lit_frames_end).collect();
            animated_sprite.repeat = true;
            animated_sprite.fps = lit_fps;
            commands.add(
                move |mut idle: CompMut<IdleKickBomb>, mut lit: CompMut<LitKickBomb>| {
                    idle.remove(entity);
                    lit.insert(
                        entity,
                        LitKickBomb {
                            arm_delay: Timer::new(arm_delay, TimerMode::Once),
                            fuse_time: Timer::new(fuse_time, TimerMode::Once),
                            kicking: false,
                            kicks: 0,
                        },
                    );
                },
            );
        }
    }
}

fn update_lit_kick_bombs(
    entities: Res<Entities>,
    kick_bomb_handles: Comp<KickBombHandle>,
    assets: Res<AssetServer>,
    collision_world: CollisionWorld,
    player_indexes: Comp<PlayerIdx>,
    mut audio_center: ResMut<AudioCenter>,
    mut trauma_events: ResMutInit<CameraTraumaEvents>,
    mut lit_grenades: CompMut<LitKickBomb>,
    mut sprites: CompMut<AtlasSprite>,
    mut bodies: CompMut<KinematicBody>,
    mut hydrated: CompMut<MapElementHydrated>,
    player_inventories: PlayerInventories,
    mut transforms: CompMut<Transform>,
    mut commands: Commands,
    time: Res<Time>,
    spawners: Comp<DehydrateOutOfBounds>,
    invincibles: CompMut<Invincibility>,
) {
    for (entity, (kick_bomb, kick_bomb_handle, spawner)) in
        entities.iter_with((&mut lit_grenades, &kick_bomb_handles, &Optional(&spawners)))
    {
        let kick_bomb_meta = assets.get(kick_bomb_handle.0);
        let KickBombMeta {
            explosion_sound,
            explosion_volume,
            explode_on_contact,
            kick_velocity,
            kickable,
            damage_region_lifetime,
            damage_region_size,
            explosion_lifetime,
            explosion_atlas,
            explosion_fps,
            explosion_frames,
            ..
        } = *kick_bomb_meta;

        kick_bomb.fuse_time.tick(time.delta());
        kick_bomb.arm_delay.tick(time.delta());

        let should_explode = 'should_explode: {
            if kick_bomb.fuse_time.finished() {
                break 'should_explode true;
            }

            if explode_on_contact {
                let players = entities
                    .iter_with(&player_indexes)
                    .map(|x| x.0)
                    .collect::<Vec<_>>();

                let colliding_with_players = collision_world
                    .actor_collisions_filtered(entity, |e| {
                        players.contains(&e) && invincibles.get(e).is_none()
                    })
                    .into_iter()
                    .collect::<Vec<_>>();

                if !colliding_with_players.is_empty() && kick_bomb.arm_delay.finished() {
                    break 'should_explode true;
                }
            }

            // If the item is being held
            if player_inventories.find_item(entity).is_some() {
                kick_bomb.kicking = false;
                break 'should_explode false;
            }

            if kickable {
                // If the item is colliding with a non-invincible player
                if let Some(player_entity) = collision_world
                    .actor_collisions_filtered(entity, |e| !invincibles.contains(e))
                    .into_iter()
                    .find(|&x| player_indexes.contains(x))
                {
                    if !std::mem::replace(&mut kick_bomb.kicking, true) {
                        kick_bomb.kicks += 1;
                    }

                    // Explode on the 3rd kick.
                    // Dropping the bomb is detected as a kick so we explode when
                    // the counter reaches 4.
                    if kick_bomb.kicks > 3 {
                        break 'should_explode true;
                    }

                    let body = bodies.get_mut(entity).unwrap();
                    let translation = transforms.get_mut(entity).unwrap().translation;

                    let player_sprite = sprites.get_mut(player_entity).unwrap();
                    let player_translation = transforms.get(player_entity).unwrap().translation;

                    let player_standing_left = player_translation.x <= translation.x;

                    if body.velocity.x == 0.0 {
                        body.velocity = kick_velocity;
                        if player_sprite.flip_x {
                            body.velocity.x *= -1.0;
                        }
                    } else if player_standing_left && !player_sprite.flip_x {
                        body.velocity.x = kick_velocity.x;
                        body.velocity.y = kick_velocity.y;
                    } else if !player_standing_left && player_sprite.flip_x {
                        body.velocity.x = -kick_velocity.x;
                        body.velocity.y = kick_velocity.y;
                    } else if kick_bomb.arm_delay.finished() {
                        break 'should_explode true;
                    }
                } else {
                    kick_bomb.kicking = false;
                }
            }
            false
        };

        // If it's time to explode
        if should_explode {
            audio_center.play_sound(explosion_sound, explosion_volume);

            trauma_events.send(7.5);

            if let Some(spawner) = spawner {
                // Cause the item to respawn by un-hydrating it's spawner.
                hydrated.remove(**spawner);
            }

            let mut explosion_transform = *transforms.get(entity).unwrap();
            explosion_transform.translation.z = -10.0; // On top of almost everything
            explosion_transform.rotation = Quat::IDENTITY;

            commands.add(
                move |mut entities: ResMutInit<Entities>,
                      mut transforms: CompMut<Transform>,
                      mut damage_regions: CompMut<DamageRegion>,
                      mut lifetimes: CompMut<Lifetime>,
                      mut sprites: CompMut<AtlasSprite>,
                      mut animated_sprites: CompMut<AnimatedSprite>| {
                    // Despawn the kick bomb
                    entities.kill(entity);

                    // Spawn the damage region
                    let ent = entities.create();
                    transforms.insert(ent, explosion_transform);
                    damage_regions.insert(
                        ent,
                        DamageRegion {
                            size: damage_region_size,
                        },
                    );
                    lifetimes.insert(ent, Lifetime::new(damage_region_lifetime));

                    // Spawn the explosion animation
                    let ent = entities.create();
                    transforms.insert(ent, explosion_transform);
                    sprites.insert(
                        ent,
                        AtlasSprite {
                            atlas: explosion_atlas,
                            ..default()
                        },
                    );
                    animated_sprites.insert(
                        ent,
                        AnimatedSprite {
                            frames: (0..explosion_frames).collect(),
                            fps: explosion_fps,
                            repeat: false,
                            ..default()
                        },
                    );
                    lifetimes.insert(ent, Lifetime::new(explosion_lifetime));
                },
            );
        }
    }
}
