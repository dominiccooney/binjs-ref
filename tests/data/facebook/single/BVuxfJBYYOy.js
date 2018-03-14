if (self.CavalryLogger) { CavalryLogger.start_js(["cM4aM"]); }

__d('EventReminderMembersMap',['CurrentUser','EventReminderConstants','Map'],(function a(b,c,d,e,f,g){'use strict';var h=c('EventReminderConstants').GuestStates;function i(){this.$EventReminderMembersMap1=new (c('Map'))();this.$EventReminderMembersMap2=new (c('Map'))();this.$EventReminderMembersMap3=new (c('Map'))();}i.prototype.setAllMembers=function(j,k){if(!j)return;this.$EventReminderMembersMap1.set(k,h.INVITED);var l=[],m=[],n=[];for(var o in j){var p=j[o];if(o===c('CurrentUser').getID())this.$EventReminderMembersMap1.set(k,p);var q={fbid:o,guestState:p,imageSrc:''};if(p===h.GOING){l.push(q);}else if(p===h.DECLINED){m.push(q);}else n.push(q);}this.$EventReminderMembersMap2.set(k,l.concat(m,n));this.$EventReminderMembersMap3.set(k,l);};i.prototype.clearMembers=function(j){this.$EventReminderMembersMap1['delete'](j);this.$EventReminderMembersMap2['delete'](j);this.$EventReminderMembersMap3['delete'](j);};i.prototype.getAllMembers=function(j){return this.$EventReminderMembersMap2.get(j);};i.prototype.getGoingMembers=function(j){return this.$EventReminderMembersMap3.get(j);};i.prototype.getSelfGuestState=function(j){if(!this.$EventReminderMembersMap1.has(j))return h.INVITED;return this.$EventReminderMembersMap1.get(j);};f.exports=i;}),null);
__d('EventReminderStateStore',['EventReminderActions','EventReminderDispatcher','EventReminderMembersMap','FluxStore'],(function a(b,c,d,e,f,g){'use strict';var h,i;h=babelHelpers.inherits(j,c('FluxStore'));i=h&&h.prototype;function j(){i.constructor.call(this,c('EventReminderDispatcher'));this.$EventReminderStateStore1={};this.$EventReminderStateStore2=new (c('EventReminderMembersMap'))();}j.prototype.getEvent=function(k){return this.$EventReminderStateStore1[k];};j.prototype.getEventMembers=function(k){return this.$EventReminderStateStore2.getAllMembers(k);};j.prototype.getSelfGuestState=function(k){return this.$EventReminderStateStore2.getSelfGuestState(k);};j.prototype.getNumOfGoingMembers=function(k){var l=this.$EventReminderStateStore2.getGoingMembers(k);return l?l.length:0;};j.prototype.clearEventMembers=function(k){this.$EventReminderStateStore2.clearMembers(k);};j.prototype.__onDispatch=function(k){var l=c('EventReminderActions').Types,m=k.event.threadID;switch(k.type){case l.UPDATE_EVENT_REMINDER:if(!m)break;var n=k.event.allowRSVP;this.$EventReminderStateStore1[m]={allowRSVP:n,eventDate:k.event.eventDate,eventName:k.event.eventName,eventLocationName:k.event.eventLocationName,eventLocationAddress:k.event.eventLocationAddress,eventID:k.event.eventID,eventType:k.event.eventType,exists:true};if(n)this.$EventReminderStateStore2.setAllMembers(k.event.eventMembers,m);break;case l.DELETE_EVENT_REMINDER:this.$EventReminderStateStore1[m].exists=false;this.clearEventMembers(m);break;}this.__emitChange();};f.exports=new j();}),null);