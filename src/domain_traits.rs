use screeps::ResourceType;

pub trait HasStore {
    fn store(&self) -> screeps::Store;
}

impl<T: screeps::HasStore> HasStore for T {
    fn store(&self) -> screeps::Store {
        screeps::HasStore::store(self)
    }
}

pub trait HasStoreExt {
    fn capacity(&self, ty: Option<ResourceType>) -> u32;
    fn used_capacity(&self, ty: Option<ResourceType>) -> u32;
    fn free_capacity(&self, ty: Option<ResourceType>) -> u32;
}

impl<T: HasStore> HasStoreExt for T {
    fn capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_capacity(ty) }
    fn used_capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_used_capacity(ty) }
    fn free_capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_free_capacity(ty).try_into().unwrap_or(0) }
}

pub trait EnergyStoreAccessors {
    fn energy_capacity(&self) -> u32;
    fn used_energy_capacity(&self) -> u32;
    fn free_energy_capacity(&self) -> u32;
}

impl<T: HasStoreExt> EnergyStoreAccessors for T {
    fn energy_capacity(&self) -> u32 { self.capacity(Some(ResourceType::Energy)) }
    fn used_energy_capacity(&self) -> u32 { self.used_capacity(Some(ResourceType::Energy)) }
    fn free_energy_capacity(&self) -> u32 { self.free_capacity(Some(ResourceType::Energy)) }
}

pub trait Transferable: HasStoreExt {
    fn transferable(&self) -> &dyn screeps::Transferable;
}

impl<T: screeps::Transferable + screeps::HasStore> Transferable for T {
    fn transferable(&self) -> &dyn screeps::Transferable { self }
}

pub trait Withdrawable: HasStoreExt {
    fn withdrawable(&self) -> &dyn screeps::Withdrawable;
}

impl<T: screeps::Withdrawable + screeps::HasStore> Withdrawable for T {
    fn withdrawable(&self) -> &dyn screeps::Withdrawable { self }
}