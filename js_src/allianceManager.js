const { encrypt, decrypt } = require("./crypt");

// alliance leader playerName. undefined to disable synchronization
const SYNC_PLAYER_BY_SHARD = {
	shard0: undefined,
	shard1: 'U-238',
	shard2: 'Winnduu',
	shard3: 'Shylo132',
	SSS: 'Shylo132',
};

const COUNCIL = 'council';
const MEMBER = 'member';
const INACTIVE = 'inactive';
const ASSOCIATE = 'associate';

const NEXT_TICK = Symbol('NEXT_TICK');
const ERR_DECRYPTION_FAILED = -1;
const ERR_JSON_PARSE_FAILED = -2;

const ALLIANCE_OPTIONS = {
	
	interval: 20, // interval of members data sync in game ticks
	
	sync: {
		enabled: true,
		playerName: SYNC_PLAYER_BY_SHARD[Game.shard.name],
		
		keySegmentId: 65, // (private) segment id for key storage
		segmentId: 66, // (public) segment id for allies synchronization
		dataSegmentId: 67, // (public) segment id for ally members data
		interval: 100, // interval of synchronization in game ticks
	},
	
	// non synced:
	// localAllies: {},
	
};

function isEmptyObject(data) {
	for (const key in data) {
		if (data[key]) {
			return false;
		}
	}
	return true;
}

class ForeignSegmentProvider {
	
	constructor() {
		this.requestedPlayerName = undefined;
		this.requestedSegmentId = undefined;
		this.dataRaw = undefined;
		this.data = undefined;
	}
	
	/**
	 * @param {boolean} raw 
	 * @returns {object | string | ERR_JSON_PARSE_FAILED}
	 */
	read(raw = false) {
		const { username, id, data } = RawMemory.foreignSegment || {};
		if (
			username !== this.requestedPlayerName ||
			id !== this.requestedSegmentId ||
			!data
		) {
			return;
		}
		try {
			this.data = raw ? undefined : JSON.parse(data);
			this.dataRaw = data;
			return raw ? this.dataRaw : this.data;
		} catch(error) {
			console.log(`ForeignSegmentProvider failed to parse foreign segment data player='${username}', segmentId=${id}: ${error.message}`);
			return ERR_JSON_PARSE_FAILED;
		}
	}
	
	/**
	 * @param {string} playerName
	 * @param {number} segmentId
	 */
	requestSegment(playerName, segmentId) {
		this.requestedPlayerName = playerName;
		this.requestedSegmentId = segmentId;
		this.data = undefined;
		this.dataRaw = undefined;
		RawMemory.setActiveForeignSegment(playerName, segmentId);
	}

	/**
	 * @param {string} playerName
	 * @param {number} segmentId
	 * @param {boolean} raw
	 * @returns {object | string | ERR_JSON_PARSE_FAILED | NEXT_TICK}
	 */
	readSegment(playerName, segmentId, raw = false) {
		if (
			this.requestedPlayerName !== playerName ||
			this.requestedSegmentId !== segmentId
		) {
			this.requestSegment(playerName, segmentId);
			return NEXT_TICK;
		}
		return this.read(raw);
	}
	
	/**
	 * @param {number} segmentId
	 * @param {object | string} data
	 * @param {boolean} raw
	 */
	writeSegment(segmentId, data, raw = false) {
		RawMemory.segments[segmentId] = raw ? data : JSON.stringify(data);
	}
	
	/**
	 * @param {number[]} segments
	 */
	setPublicSegments(segments) {
		RawMemory.setPublicSegments(segments);
	}
	
	/**
	 * @param {number} segmentId
	 * @param {object} data
	 * @param {string} key
	 */
	writeEncryptedSegment(segmentId, data, key) {
		const s = JSON.stringify(data);
		this.writeSegment(segmentId, s ? encrypt(s, key) : '', true);
	}
	
	/**
	 * @param {string} playerName
	 * @param {number} segmentId
	 * @param {string} key
	 * @returns {object | NEXT_TICK | ERR_JSON_PARSE_FAILED | ERR_DECRYPTION_FAILED}
	 */
	readEncryptedSegment(playerName, segmentId, key) {
		const data = this.readSegment(playerName, segmentId, true);
		if (data === '') {
			return {};
		}
		if (data === NEXT_TICK || !data) {
			return data;
		}
		try {
			const s = decrypt(data, key);
			if (s.charAt(0) !== '{') {
				return ERR_DECRYPTION_FAILED;
			}
			return JSON.parse(s);
		} catch (error) {
			console.log(`ForeignSegmentProvider failed to parse foreign segment data player='${playerName}', segmentId=${segmentId}: ${error.message}`);
			return ERR_JSON_PARSE_FAILED;
		}
	}
	
}

class AllianceDataSync {
	
	/**
	 * 
	 * @param {ForeignSegmentProvider} foreignSegmentProvider
	 * @param {() => object} getData
	 * @param {(username: string) => boolean} isAlly
	 * @param {Function} onKeyChanged
	 * @param {object} options
	 */
	constructor(foreignSegmentProvider, getData, isAlly, onKeyChanged, options = {}) {
		this.segmentProvider = foreignSegmentProvider;
		this.getData = getData;
		this.isAlly = isAlly;
		this.onKeyChanged = onKeyChanged;
		this.leaderPlayerName = options.playerName;
		this.keySegmentId = options.keySegmentId;
		this.segmentId = options.segmentId;
		this.dataSegmentId = options.dataSegmentId;
		this.interval = options.interval || 100;
		this.keyData = undefined;
		this.nextSyncTime = Game.time;
		this.leaderRoomName = undefined;
		this.isEnabled = Boolean(this.leaderPlayerName);

		this.segmentProvider.setPublicSegments([this.dataSegmentId]);
	}
	
	/**
	 * @param {number[]} segments
	 */
	setPublicSegments(segments = []) {
		const allianceSegments = [this.dataSegmentId];
		this.segmentProvider.setPublicSegments(
			(segments.length > 0) ? allianceSegments.concat(segments) : allianceSegments
		);
	}
	
	/**
	 * @param {number[]} segments
	 */
	setActiveSegments(segments) {
		RawMemory.setActiveSegments(
			this.keyData ? segments : segments.concat([this.keySegmentId])
		);
	}
	
	/**
	 * @returns {boolean}
	 */
	readKeyFromTransactions() {
		let i = 0;
		for (const transaction of Game.market.incomingTransactions) {
			if (Game.time > transaction.time + 1000) {
				break;
			}
			if (
				transaction.sender && transaction.sender.username === this.leaderPlayerName &&
				transaction.resourceType === RESOURCE_ENERGY &&
				transaction.amount === 3
			) {
				if (transaction.description.length === 66) {
					this.keyData.newKey = transaction.description.slice(2);
					this.saveKeyData();
					return true;
				}
				if (transaction.description.length === 64) {
					this.keyData.key = transaction.description;
					this.saveKeyData();
					return true;
				}
			}
			i++;
			if (i >= 30) {
				break;
			}
		}
		return false;
	}
	
	/**
	 * @returns {boolean}
	 */
	readKeyFromLocalSegment() {
		const data = RawMemory.segments[this.keySegmentId];
		if (data === undefined) {
			RawMemory.setActiveSegments([this.keySegmentId]);
			return false;
		}
		this.keyData = JSON.parse(data || 'null') || {key: undefined, expire: null};
		return true;
	}
	
	saveKeyData() {
		RawMemory.segments[this.keySegmentId] = JSON.stringify(this.keyData);
	}

	/**
	 * @param {object} data 
	 */
	publishAllies(data) {
		this.segmentProvider.writeEncryptedSegment(this.segmentId, data, this.keyData.key);
	}
	
	/**
	 * @param {Room} room
	 * @returns {StructureTerminal | undefined}
	 */
	getValidTerminal(room) {
		if (
			room.controller && room.controller.my &&
			room.terminal && room.terminal.my
			// prob check `room.terminal.isActive`
		) {
			return room.terminal;
		}
	}
	
	
	/**
	 * @param {number} expire
	 * @param {string} leaderRoom
	 */
	setKeyData(expire, leaderRoom) {
		if (
			expire === this.keyData.expire &&
			leaderRoom === this.keyData.leaderRoom
		) {
			return;
		}
		this.keyData.expire = expire;
		this.keyData.leaderRoom = leaderRoom;
		this.saveKeyData();
	}

	clearKey() {
		this.keyData.key = undefined;
		this.saveKeyData();
	}
	
	/**
	 * @returns {boolean}
	 */
	updateExpiredKey() {
		if (this.keyData.key && this.keyData.expire && Game.time >= this.keyData.expire) {
			this.keyData.key = this.keyData.newKey; // intended that it might be undefined
			this.keyData.newKey = undefined;
			this.keyData.expire = null;
			this.saveKeyData();
			if (this.keyData.key && this.onKeyChanged) {
				this.onKeyChanged();
			}
			return true;
		}
		return false;
	}
	
	/**
	 * @param {string} leaderRoom
	 */
	setLeaderRoom(leaderRoom) {
		this.keyData.leaderRoom = leaderRoom;
		this.saveKeyData();
	}
	
	/**
	 * @returns {NEXT_TICK | undefined}
	 */
	requestKey() {
		if (this.keyData.newKey) {
			return;
		}
		if (this.readKeyFromTransactions()) {
			return NEXT_TICK;
		}
		
		if (!this.keyData.leaderRoom) {
			console.log(`AllianceDataSync leader room is undefined`);
			console.log(`type in game console: Alliance.setLeaderRoom('<leader room name here...>')`);
			return NEXT_TICK;
		}
		
		let hasValidTerminal = false;
		for (const name in Game.rooms) {
			const terminal = this.getValidTerminal(Game.rooms[name]);
			if (terminal) {
				hasValidTerminal = true;
				if (
					terminal.cooldown === 0 && terminal.store.energy >= 10 &&
					terminal.send(RESOURCE_ENERGY, 3, this.keyData.leaderRoom) === OK
				) {
					break;
				}
			}
		}
		if (!hasValidTerminal) {
			console.log(`AllianceDataSync you don't have a valid terminal`);
			return NEXT_TICK;
		}
	}
	
	/**
	 * @param {object} data
	 */
	writeMyData(data) {
		if (!this.keyData.key) {
			return;
		}
		const _data = isEmptyObject(data)
			? undefined : data;
		this.segmentProvider.writeEncryptedSegment(this.dataSegmentId, _data, this.keyData.key);
	}

	/**
	 * @param {string} playerName
	 * @returns {object | NEXT_TICK | undefined}
	 */
	readAllyData(playerName) {
		if (!this.keyData.key) {
			return;
		}
		
		const data = this.segmentProvider.readEncryptedSegment(
			playerName, this.dataSegmentId, this.keyData.key
		);
		if (data === NEXT_TICK || !data) {
			return data;
		}
		if (Object.keys(data).length === 0) {
			return;
		}		
		return data;
	}
	
	/**
	 * @returns {object | NEXT_TICK | undefined}
	 */
	syncMember() {
		if (!this.keyData && !this.readKeyFromLocalSegment()) {
			return NEXT_TICK;
		}
		if (this.updateExpiredKey()) {
			return NEXT_TICK;
		}
		
		if (
			!this.keyData.key ||
			(this.keyData.expire && Game.time >= this.keyData.expire - 1000)
		) {
			return this.requestKey();
		}

		const useNewKey = (this.keyData.newKey && !this.keyData.expire);
		const data = this.segmentProvider.readEncryptedSegment(
			this.leaderPlayerName, this.segmentId,
			useNewKey ? this.keyData.newKey : this.keyData.key
		);
		if (data === NEXT_TICK || !data) {
			return NEXT_TICK;
		}
		if (data === ERR_JSON_PARSE_FAILED) {
			return;
		}
		if (data === ERR_DECRYPTION_FAILED) { // key changed
			if (useNewKey) {
				return;
			}
			this.keyData = {
				key: undefined,
				leaderRoom: this.keyData.leaderRoom,
				expire: null
			};
			this.saveKeyData();
			return NEXT_TICK;
		}
		if (data.keyExpireTime || data.room) {
			this.setKeyData(data.keyExpireTime || null, data.room);
		}
		return data;
	}
	
	/**
	 * @returns {object | NEXT_TICK | undefined}
	 */
	sync() {
		if (Game.time < this.nextSyncTime) {
			return;
		}
		const data = this.syncMember();
		if (data !== NEXT_TICK) {
			this.nextSyncTime = Game.time + this.interval;
		}
		return data;
	}
	
}

class AllianceManager {
	
	/**
	 * @param {object} options
	 */
	constructor(options = {}) {
		this.interval = options.interval;
		this.localAllies = options.localAllies || {};
		this.allies = Memory.allies || {};
		this._alliesArray = undefined;
		this.myUserName = this.getMyUserName();
		
		this.segmentProvider = options.foreignSegmentProvider || new ForeignSegmentProvider();

		const syncOptions = options.sync || {};
		this.dataSync = syncOptions.enabled
			? new AllianceDataSync(
				this.segmentProvider,
				() => ({ allies: this.allies }),
				(u) => u in this.allies,
				() => this.saveMyData(),
				syncOptions
			)
			: undefined;

		this.myData = Memory.myData || {};
		this.alliesData = Memory.alliesData || {};
		this.updateSyncDataAllies();
	}
	
	/**
	 * @returns {string | undefined}
	 */
	getMyUserName() {
		for (const name in Game.rooms) {
			const room = Game.rooms[name];
			if (room.controller && room.controller.my) {
				return room.controller.owner.username;
			}
		}
	}
	
	updateSyncDataAllies() {
		this.syncDataIndex = 0;
		this.syncDataAllies = Object.keys(this.allies)
			.filter( playerName => (
				playerName !== this.myUserName &&
				this.hasPermissionPublishRequests(this.allies[playerName])
			));
		// clean allies data that have no permissions to publish:
		for (const playerName in this.alliesData) {
			if (!this.hasPermissionPublishRequests(this.allies[playerName])) {
				this.alliesData[playerName] = undefined;
			}
		}
	}
	
	saveMyData() {
		if (this.dataSync) {
			this.dataSync.writeMyData(this.myData);
		}
	}

	
	/**
	 * @param {number[]} segments
	 */
	setActiveSegments(segments) {
		if (!this.dataSync) {
			RawMemory.setActiveSegments(segments);
			return;
		}
		this.dataSync.setActiveSegments(segments);
	}

	/**
	 * @param {number[]} segments
	 */
	setPublicSegments(segments = []) {
		if (!this.dataSync) {
			RawMemory.setPublicSegments(segments);
			return;
		}
		this.dataSync.setPublicSegments(segments);
	}

	/**
	 * @param {string} leaderRoom
	 */
	setLeaderRoom(leaderRoom) {
		if (this.dataSync) {
			this.dataSync.setLeaderRoom(leaderRoom);
		}
	}
	
	/**
	 * @param {string} playerName
	 * @returns {boolean}
	 */
	isAlly(playerName) {
		return (
			playerName in this.allies ||
			playerName in this.localAllies
		);
	}

	/**
	 * @param {string[]} localAllies
	 */
	setLocalAllies(localAllies) {
		this.localAllies = localAllies;
		this._alliesArray = undefined;
	}

	/**
	 * @returns {string[]}
	 */
	getAlliesArray() {
		if (!this._alliesArray) {
			this._alliesArray = [...Object.keys(this.allies), ...Object.keys(this.localAllies)];
		}
		return this._alliesArray;
	}
	
	/**
	 * @param {string} playerName
	 * @returns {COUNCIL | MEMBER | INACTIVE | ASSOCIATE}
	 */
	getAllyStatus(playerName) {
		return this.allies[playerName];
	}
	
	/**
	 * @param {string} playerName
	 * @returns {object}
	 */
	getAllyData(playerName) {
		return this.alliesData[playerName] || {};
	}

	/**
	 * @param {string} type
	 * @returns {any[]}
	 */
	getAlliesRequests(type) {
		if (type === 'econ') {
			console.log(`AllianceManager "econ" requests should be requested via "getAllyRequests(playerName, 'econ')"`);
			return;
		}
		const requests = [];
		for (const playerName in this.alliesData) {
			const data = this.alliesData[playerName];
			if (!data || !data[type]) {
				continue;
			}
			requests.push(...data[type]);
		}
		return requests;
	}
	
	/**
	 * @param {string} playerName
	 * @param {string} type
	 * @returns {any[] | object}
	 */
	getAllyRequests(playerName, type) {
		const data = this.alliesData[playerName] || {};
		if (type === 'econ') {
			return data[type];
		}
		return data[type] || [];
	}

	
	/**
	 * @param {COUNCIL | MEMBER | INACTIVE | ASSOCIATE} allyStatus
	 * @returns {boolean}
	 */
	hasPermissionPublishRequests(allyStatus) {
		return allyStatus === COUNCIL || allyStatus === MEMBER;
	}
	
	/**
	 * @param {object} data
	 * @param {COUNCIL | MEMBER | INACTIVE | ASSOCIATE} allyStatus
	 * @returns {object}
	 */
	applyPermissions(data, allyStatus) {
		if (!data) {
			return;
		}
		if (allyStatus === MEMBER) {
			data.attack = undefined;
			data.player = undefined;
		}
		return data;
	}

	
	/**
	 * @returns {boolean}
	 */
	syncAllies() {
		const data = this.dataSync.sync();
		if (data === NEXT_TICK) {
			return false;
		}
		if (data && data.allies) {
			Memory.allies = this.allies = data.allies;
			this._alliesArray = undefined;
			this.updateSyncDataAllies();
		}
		return true;
	}
	
	syncAlliesData() {
		if (Game.time < this.nextDataSync) {
			return;
		}
		
		const playerName = this.syncDataAllies[this.syncDataIndex];
		let data = this.dataSync.readAllyData(playerName);
		if (data === NEXT_TICK) {
			return;
		}
		
		if (data === ERR_DECRYPTION_FAILED) {
			console.log(`AllianceDataSync foreign segment decryption failed player='${playerName}', segmentId=${this.dataSync.dataSegmentId}`);
		} else if (data !== ERR_JSON_PARSE_FAILED) {
			this.alliesData[playerName] = this.applyPermissions(data, this.getAllyStatus(playerName));
			// console.log(`AllianceManager: Synchronization for player ${playerName} complete`);
			Memory.alliesData = this.alliesData;
		}

		this.syncDataIndex++;
		if (this.syncDataIndex >= this.syncDataAllies.length) {
			this.syncDataIndex = 0;
		}
		if (data !== undefined || this.syncDataIndex === 0) {
			this.nextDataSync = Game.time + this.interval;
		}
	}
	
	sync() {
		Memory.myData = this.myData;
		if (!this.dataSync || !this.dataSync.isEnabled) {
			return;
		}
		
		if (!this.syncAllies()) {
			return;
		}
		
		this.syncAlliesData();
	}
	
	
	// Request methods
	/**
	 * @param {string} type
	 * @param {object} request
	 */
	addRequest(type, request) {
		const requests = this.myData[type] || (this.myData[type] = []);
		requests.push(request);
		this.saveMyData();
	}
	
	/**
	 * @param {object} data
	 */
	setMyData(data) {
		Memory.myData = this.myData = data;
		this.saveMyData();
	}
	
	/**
	 * Request resource
	 * @param {Object} args - a request object
	 * @param {number} args.priority - 0-1 where 1 is highest consideration
	 * @param {string} args.roomName
	 * @param {ResourceConstant} args.resourceType
	 * @param {number} args.amount - How much they want of the resource. If the responder sends only a portion of what you ask for, that's fine
	 * @param {boolean} [args.terminal] - If the bot has no terminal, allies should instead haul the resources to us
	 */
	requestResource(args) {
		this.addRequest('resource', args);
	}
	
	/**
	 * Request help in defending a room
	 * @param {Object} args - a request object
	 * @param {number} args.priority - 0-1 where 1 is highest consideration
	 * @param {string} args.roomName
	 */
	requestDefense(args) {
		this.addRequest('defense', args);
	}
	
	/**
	 * Request an attack on a specific room
	 * @param {Object} args - a request object
	 * @param {number} args.priority - 0-1 where 1 is highest consideration
	 * @param {string} args.roomName
	 */
	requestAttack(args) {
		this.addRequest('attack', args);
	}
	
	/**
	 * Influence allies aggression score towards a player
	 * @param {Object} args - a request object
	 * @param {number} args.playerName - name of a player
	 * @param {number} args.hate - 0-1 where 1 is highest consideration. How much you think your team should hate the player. Should probably affect combat aggression and targetting
	 * @param {number} args.lastAttackedBy - The last time this player has attacked you
	 */
	sharePlayer(args) {
		this.addRequest('player', args);
	}
	
	/**
	 * Request help in building/fortifying a room
	 * @param {Object} args - a request object
	 * @param {string} args.roomName
	 * @param {number} args.priority - 0-1 where 1 is highest consideration
	 * @param {'build' | 'repair'} args.workType
	 */
	requestWork(args) {
		this.addRequest('work', args);
	}
	
	/**
	 * Request energy to a room for a purpose of making upgrading faster.
	 * @param {Object} args - a request object
	 * @param {number} args.maxAmount - Amount of energy needed. Should be equal to energy that needs to be put into controller for achieving goal.
	 * @param {EFunnelGoalType['GCL'] | EFunnelGoalType['RCL7'] | EFunnelGoalType['RCL8']} args.goalType - What energy will be spent on. Room receiving energy should focus solely on achieving the goal.
	 * @param {string} [args.roomName] - Room to which energy should be sent. If undefined resources can be sent to any of requesting player's rooms.
	 */
	requestFunnel(args) {
		this.addRequest('funnel', args);
	}
	
	/**
	 * Share how your bot is doing economically
	 * @param {Object} args - a request object
	 * @param {number} args.credits - total credits the bot has. Should be 0 if there is no market on the server
	 * @param {number} args.sharableEnergy - the maximum amount of energy the bot is willing to share with allies. Should never be more than the amount of energy the bot has in storing structures
	 * @param {number} [args.energyIncome] - The average energy income the bot has calculated over the last 100 ticks. Optional, as some bots might not be able to calculate this easily.
	 * @param {Object.<MineralConstant, number>} [args.mineralNodes] - The number of mineral nodes the bot has access to, probably used to inform expansion
	 */
	shareEcon(args) {
		this.myData.econ = args;
	}
	
	/**
	 * Share scouting data about hostile owned rooms
	 * @param {Object} args - a request object
	 * @param {string} args.roomName
	 * @param {string} args.playerName - The player who owns this room. If there is no owner, the room probably isn't worth making a request about
	 * @param {number} args.lastScout - The last tick your scouted this room to acquire the data you are now sharing
	 * @param {number} args.rcl
	 * @param {number} args.energy - The amount of stored energy the room has. storage + terminal + factory should be sufficient
	 * @param {number} args.towers
	 * @param {number} args.avgRamprtHits
	 * @param {boolean} args.terminal - does scouted room have terminal built
	 */
	shareRoom(args) {
		this.addRequest('room', args);
	}
	
}

global.Alliance = global.Alliance || new AllianceManager(ALLIANCE_OPTIONS);
module.exports = global.Alliance;

if (!StructureTerminal.prototype.sendOriginal) {
	StructureTerminal.prototype.sendOriginal = StructureTerminal.prototype.send;
	StructureTerminal.prototype.send = function(resourceType, amount, destination, description = undefined) {
		if (this.isUsed) {
			return ERR_TIRED;
		}
		const res = this.sendOriginal(resourceType, amount, destination, description);
		if (res === OK) {
			this.isUsed = true;
		}
		return res;
	};
}